use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufReader, Read, Write};
use std::net::TcpStream;
use std::sync::Mutex;
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::ai::AiCommanders;
use crate::catalog::Team;
use crate::determinism::simulation_checksum;
use crate::i18n::{tr, tr_fmt};
use crate::levels::LevelDefinition;
use crate::notifications::Notifications;
use crate::player_commands::{
    CommandQueue, LocalTeam, PlayerCommand, ScheduledCommand, SimulationTick,
};

/// Commands wait this many ticks (200 ms) before they run, so they reach the other machine in time.
pub const NETWORK_COMMAND_DELAY_TICKS: u64 = 4;
const CHECKSUM_EVERY_TICKS: u64 = 20;
const LARGEST_MESSAGE_BYTES: usize = 1 << 20;
const PING_INTERVAL: Duration = Duration::from_secs(1);
const PINGS_REMEMBERED: u64 = 10;
const ROUND_TRIP_SMOOTHING: f32 = 0.3;

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
enum Message {
    /// What a player ordered for one tick, possibly nothing.
    Turn {
        tick: u64,
        commands: Vec<PlayerCommand>,
    },
    /// The fingerprint of the battle after a tick, sent as soon as it is known so a drift is noticed even
    /// when the machine that finds it first stops at once.
    Checksum {
        tick: u64,
        sum: u64,
    },
    /// The host's choice of battle, sent once the guest has connected.
    Offer {
        version: String,
        level: Box<LevelDefinition>,
    },
    /// The guest accepts the offered battle.
    Ready,
    /// Asks the other machine to answer at once, to measure the round trip.
    Ping {
        nonce: u64,
    },
    Pong {
        nonce: u64,
    },
    /// A line the other player typed, in the lobby or during the battle.
    Chat {
        text: String,
    },
    /// The host starts the countdown to the battle.
    Go,
}

/// What happened in the lobby since the last look.
#[derive(Clone, PartialEq, Debug)]
pub enum LobbyNews {
    /// The host's battle; `Err` carries the host's version when it differs from this one.
    Offered(Result<LevelDefinition, String>),
    GuestReady,
    Go,
    Left,
}

enum Arrival {
    Message(Message),
    ConnectionLost,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SessionState {
    InStep,
    /// The two machines computed different battles at this tick; play cannot continue.
    Desynced(u64),
    Disconnected,
}

/// A match against a player on another machine: both run the whole simulation and exchange only commands.
#[derive(Resource)]
pub struct NetSession {
    writer: Mutex<TcpStream>,
    inbox: Mutex<Receiver<Arrival>>,
    /// Messages that arrived while the guest was still waiting for the battle to start.
    early: Vec<Arrival>,
    remote_team: Team,
    remote_turns: BTreeSet<u64>,
    local_checksums: BTreeMap<u64, u64>,
    remote_checksums: BTreeMap<u64, u64>,
    pub state: SessionState,
    reported: Option<SessionState>,
    pings_in_flight: BTreeMap<u64, Instant>,
    incoming_chat: Vec<String>,
    next_ping: u64,
    last_ping_sent: Option<Instant>,
    round_trip: Option<Duration>,
}

impl NetSession {
    /// Plays against `remote_team`, commanded from the other end of an established connection.
    pub fn over(stream: TcpStream, remote_team: Team) -> std::io::Result<Self> {
        stream.set_nodelay(true)?;
        let reader = stream.try_clone()?;
        let (sender, receiver) = channel();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(reader);
            loop {
                match read_message(&mut reader) {
                    Some(message) => {
                        if sender.send(Arrival::Message(message)).is_err() {
                            return;
                        }
                    }
                    None => {
                        let _ = sender.send(Arrival::ConnectionLost);
                        return;
                    }
                }
            }
        });
        Ok(Self {
            writer: Mutex::new(stream),
            inbox: Mutex::new(receiver),
            early: Vec::new(),
            remote_team,
            remote_turns: BTreeSet::new(),
            local_checksums: BTreeMap::new(),
            remote_checksums: BTreeMap::new(),
            state: SessionState::InStep,
            reported: None,
            pings_in_flight: BTreeMap::new(),
            incoming_chat: Vec::new(),
            next_ping: 0,
            last_ping_sent: None,
            round_trip: None,
        })
    }

    pub fn remote_team(&self) -> Team {
        self.remote_team
    }

    /// Sends a chat line; it is trimmed to a sensible length and stripped of control characters.
    pub fn say(&mut self, text: &str) -> Option<String> {
        let line = chat_line(text)?;
        self.send(&Message::Chat { text: line.clone() });
        Some(line)
    }

    /// Chat lines the other player sent since the last call.
    pub fn take_chat(&mut self) -> Vec<String> {
        let mut lines = std::mem::take(&mut self.incoming_chat);
        if let Ok(inbox) = self.inbox.lock() {
            self.early.extend(inbox.try_iter());
        }
        let early = std::mem::take(&mut self.early);
        for arrival in early {
            match arrival {
                Arrival::Message(Message::Chat { text }) => lines.extend(chat_line(&text)),
                other => self.early.push(other),
            }
        }
        lines
    }

    /// The smoothed time a message takes to the other machine and back; `None` until first measured.
    pub fn round_trip(&self) -> Option<Duration> {
        self.round_trip
    }

    fn ping_if_due(&mut self) {
        let due = self
            .last_ping_sent
            .is_none_or(|sent| sent.elapsed() >= PING_INTERVAL);
        if !due {
            return;
        }
        let nonce = self.next_ping;
        self.next_ping += 1;
        let now = Instant::now();
        self.last_ping_sent = Some(now);
        self.pings_in_flight.insert(nonce, now);
        self.pings_in_flight
            .retain(|&sent, _| sent + PINGS_REMEMBERED > nonce);
        self.send(&Message::Ping { nonce });
    }

    fn answered(&mut self, nonce: u64) {
        let Some(sent) = self.pings_in_flight.remove(&nonce) else {
            return;
        };
        let measured = sent.elapsed();
        self.round_trip = Some(match self.round_trip {
            Some(previous) => {
                previous.mul_f32(1.0 - ROUND_TRIP_SMOOTHING)
                    + measured.mul_f32(ROUND_TRIP_SMOOTHING)
            }
            None => measured,
        });
    }

    /// Shows the guest the host's battle.
    pub fn offer(&mut self, level: &LevelDefinition) {
        self.send(&Message::Offer {
            version: env!("CARGO_PKG_VERSION").to_string(),
            level: Box::new(level.clone()),
        });
    }

    pub fn accept_offer(&mut self) {
        self.send(&Message::Ready);
    }

    pub fn go(&mut self) {
        self.send(&Message::Go);
    }

    /// Lobby messages since the last call; battle messages that arrive early are kept for the battle.
    pub fn lobby_news(&mut self) -> Vec<LobbyNews> {
        let mut news = Vec::new();
        for arrival in self.drain_inbox() {
            match arrival {
                Arrival::Message(Message::Offer { version, level }) => {
                    news.push(LobbyNews::Offered(
                        if version == env!("CARGO_PKG_VERSION") {
                            Ok(*level)
                        } else {
                            Err(version)
                        },
                    ));
                }
                Arrival::Message(Message::Ready) => news.push(LobbyNews::GuestReady),
                Arrival::Message(Message::Go) => news.push(LobbyNews::Go),
                Arrival::Message(Message::Ping { nonce }) => self.send(&Message::Pong { nonce }),
                Arrival::Message(Message::Chat { text }) => {
                    self.incoming_chat.extend(chat_line(&text))
                }
                Arrival::ConnectionLost => {
                    self.state = SessionState::Disconnected;
                    news.push(LobbyNews::Left);
                }
                battle => self.early.push(battle),
            }
        }
        news
    }

    fn drain_inbox(&mut self) -> Vec<Arrival> {
        let mut arrivals = std::mem::take(&mut self.early);
        if let Ok(inbox) = self.inbox.lock() {
            arrivals.extend(inbox.try_iter());
        }
        arrivals
    }

    /// Whether the other player's commands for `tick` have arrived; the first ticks need none.
    fn has_turn_for(&self, tick: u64) -> bool {
        tick < NETWORK_COMMAND_DELAY_TICKS || self.remote_turns.contains(&tick)
    }

    fn send(&mut self, message: &Message) {
        let Ok(bytes) = serde_json::to_vec(message) else {
            return;
        };
        let sent = self.writer.lock().map_err(|_| ()).and_then(|mut stream| {
            stream
                .write_all(&(bytes.len() as u32).to_be_bytes())
                .and_then(|_| stream.write_all(&bytes))
                .map_err(|_| ())
        });
        if sent.is_err() {
            self.state = SessionState::Disconnected;
        }
    }

    /// Once both fingerprints of a tick are known they are compared and forgotten.
    fn compare_checksums(&mut self, tick: u64) {
        let (Some(&local), Some(&remote)) = (
            self.local_checksums.get(&tick),
            self.remote_checksums.get(&tick),
        ) else {
            return;
        };
        self.local_checksums.remove(&tick);
        self.remote_checksums.remove(&tick);
        if local != remote && self.state == SessionState::InStep {
            self.state = SessionState::Desynced(tick);
        }
    }
}

/// The reading thread holds its own handle on the connection, so leaving must close it outright for the
/// other player to notice.
impl Drop for NetSession {
    fn drop(&mut self) {
        if let Ok(stream) = self.writer.lock() {
            let _ = stream.shutdown(std::net::Shutdown::Both);
        }
    }
}

const LONGEST_CHAT_LINE: usize = 120;

/// What may be shown of a chat line: printable characters, trimmed, at most `LONGEST_CHAT_LINE` of them.
fn chat_line(text: &str) -> Option<String> {
    let clean: String = text
        .chars()
        .filter(|character| !character.is_control())
        .take(LONGEST_CHAT_LINE)
        .collect();
    let clean = clean.trim();
    (!clean.is_empty()).then(|| clean.to_string())
}

fn read_message(reader: &mut impl Read) -> Option<Message> {
    let mut length = [0; 4];
    reader.read_exact(&mut length).ok()?;
    let length = u32::from_be_bytes(length) as usize;
    if length > LARGEST_MESSAGE_BYTES {
        return None;
    }
    let mut bytes = vec![0; length];
    reader.read_exact(&mut bytes).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// The simulation may take its next step only once the other player's commands for it are known.
pub fn simulation_may_advance(session: Option<Res<NetSession>>, tick: Res<SimulationTick>) -> bool {
    session
        .is_none_or(|session| session.state == SessionState::InStep && session.has_turn_for(tick.0))
}

pub fn receive_turns(session: Option<ResMut<NetSession>>, mut queue: ResMut<CommandQueue>) {
    let Some(mut session) = session else { return };
    let arrivals = session.drain_inbox();
    let remote_team = session.remote_team;
    for arrival in arrivals {
        match arrival {
            Arrival::Message(Message::Turn { tick, commands }) => {
                for command in commands {
                    queue.schedule(ScheduledCommand {
                        tick,
                        team: remote_team,
                        command,
                    });
                }
                session.remote_turns.insert(tick);
            }
            Arrival::Message(Message::Checksum { tick, sum }) => {
                session.remote_checksums.insert(tick, sum);
                session.compare_checksums(tick);
            }
            Arrival::Message(Message::Ping { nonce }) => session.send(&Message::Pong { nonce }),
            Arrival::Message(Message::Pong { nonce }) => session.answered(nonce),
            Arrival::Message(Message::Chat { text }) => {
                session.incoming_chat.extend(chat_line(&text))
            }
            Arrival::Message(Message::Offer { .. } | Message::Ready | Message::Go) => {}
            Arrival::ConnectionLost => session.state = SessionState::Disconnected,
        }
    }
}

pub fn measure_latency(session: Option<ResMut<NetSession>>) {
    if let Some(mut session) = session
        && session.state == SessionState::InStep
    {
        session.ping_if_due();
    }
}

/// Sends this player's commands for the tick that is `NETWORK_COMMAND_DELAY_TICKS` ahead.
pub fn send_turn(
    session: Option<ResMut<NetSession>>,
    queue: Res<CommandQueue>,
    tick: Res<SimulationTick>,
    local: Res<LocalTeam>,
) {
    let Some(mut session) = session else { return };
    let turn = tick.0 + NETWORK_COMMAND_DELAY_TICKS;
    let message = Message::Turn {
        tick: turn,
        commands: queue.scheduled_for(turn, local.0),
    };
    session.send(&message);
    session.remote_turns.retain(|&received| received >= tick.0);
}

pub fn fingerprint_tick(world: &mut World) {
    let tick = world.resource::<SimulationTick>().0;
    if !world.contains_resource::<NetSession>() || !tick.is_multiple_of(CHECKSUM_EVERY_TICKS) {
        return;
    }
    let sum = simulation_checksum(world);
    let mut session = world.resource_mut::<NetSession>();
    session.local_checksums.insert(tick, sum);
    session.send(&Message::Checksum { tick, sum });
    session.compare_checksums(tick);
}

/// Tells the player once why the battle stopped.
pub fn report_session_trouble(
    session: Option<ResMut<NetSession>>,
    mut notifications: ResMut<Notifications>,
) {
    let Some(mut session) = session else { return };
    if session.state == SessionState::InStep || session.reported == Some(session.state) {
        return;
    }
    notifications.push(match session.state {
        SessionState::Disconnected => tr("The other player has left the battle").to_string(),
        SessionState::Desynced(tick) => {
            tr_fmt("Out of step with the other player at tick {}", &[&tick])
        }
        SessionState::InStep => return,
    });
    session.reported = Some(session.state);
}

/// Teams played by people on either machine get no computer commander.
pub fn retire_commanders_of_players(
    session: Option<Res<NetSession>>,
    local: Res<LocalTeam>,
    mut commanders: ResMut<AiCommanders>,
) {
    let Some(session) = session else { return };
    let humans = [local.0, session.remote_team];
    commanders
        .0
        .retain(|commander| !humans.contains(&commander.team));
}
