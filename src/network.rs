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

/// Commands wait this many ticks (200 ms) before they run, so they reach every other machine in time.
pub const NETWORK_COMMAND_DELAY_TICKS: u64 = 4;
const CHECKSUM_EVERY_TICKS: u64 = 20;
const LARGEST_MESSAGE_BYTES: usize = 1 << 20;
const PING_INTERVAL: Duration = Duration::from_secs(1);
const PINGS_REMEMBERED: u64 = 10;
const ROUND_TRIP_SMOOTHING: f32 = 0.3;
const LONGEST_CHAT_LINE: usize = 120;

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
enum Message {
    /// What one team's player ordered for one tick, possibly nothing.
    Turn {
        team: Team,
        tick: u64,
        commands: Vec<PlayerCommand>,
    },
    /// The fingerprint of one machine's battle after a tick, sent as soon as it is known so a drift is
    /// noticed even when the machine that finds it first stops at once.
    Checksum {
        team: Team,
        tick: u64,
        sum: u64,
    },
    /// The host's battle, the team this guest will play (none for a spectator), and every team people play.
    Offer {
        version: String,
        level: Box<LevelDefinition>,
        your_team: Option<Team>,
        players: Vec<Team>,
    },
    /// The guest accepts the offered battle.
    Ready,
    /// The host starts the countdown to the battle.
    Go,
    /// Asks the other machine to answer at once, to measure the round trip.
    Ping {
        nonce: u64,
    },
    Pong {
        nonce: u64,
    },
    /// A line a player typed, in the lobby or during the battle.
    Chat {
        team: Team,
        text: String,
    },
}

/// The battle a guest was offered.
#[derive(Clone, PartialEq, Debug)]
pub struct Offer {
    pub level: LevelDefinition,
    /// None when every seat is taken and the guest may only watch.
    pub your_team: Option<Team>,
    pub players: Vec<Team>,
}

/// What happened in the lobby on one connection since the last look.
#[derive(Clone, PartialEq, Debug)]
pub enum LobbyNews {
    /// The host's battle; `Err` carries the host's version when it differs from this one.
    Offered(Result<Box<Offer>, String>),
    GuestReady,
    Go,
    Chat(Team, String),
    Left,
}

enum Arrival {
    Message(Message),
    ConnectionLost,
}

/// One connection to another machine, read on its own thread.
pub struct Link {
    writer: Mutex<TcpStream>,
    inbox: Mutex<Receiver<Arrival>>,
    /// Battle messages that arrived while the lobby was still being set up.
    early: Vec<Arrival>,
}

impl Link {
    pub fn new(stream: TcpStream) -> std::io::Result<Self> {
        stream.set_nodelay(true)?;
        let reader = stream.try_clone()?;
        let (sender, receiver) = channel();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(reader);
            loop {
                let arrival = match read_message(&mut reader) {
                    Some(message) => Arrival::Message(message),
                    None => Arrival::ConnectionLost,
                };
                let lost = matches!(arrival, Arrival::ConnectionLost);
                if sender.send(arrival).is_err() || lost {
                    return;
                }
            }
        });
        Ok(Self {
            writer: Mutex::new(stream),
            inbox: Mutex::new(receiver),
            early: Vec::new(),
        })
    }

    /// Returns whether the message left; a failure means the connection is gone.
    fn send(&self, message: &Message) -> bool {
        let Ok(bytes) = serde_json::to_vec(message) else {
            return true;
        };
        self.writer.lock().is_ok_and(|mut stream| {
            stream
                .write_all(&(bytes.len() as u32).to_be_bytes())
                .and_then(|_| stream.write_all(&bytes))
                .is_ok()
        })
    }

    fn drain(&mut self) -> Vec<Arrival> {
        let mut arrivals = std::mem::take(&mut self.early);
        if let Ok(inbox) = self.inbox.lock() {
            arrivals.extend(inbox.try_iter());
        }
        arrivals
    }

    /// Shows a guest the host's battle.
    pub fn offer(&self, level: &LevelDefinition, your_team: Option<Team>, players: &[Team]) {
        self.send(&Message::Offer {
            version: env!("CARGO_PKG_VERSION").to_string(),
            level: Box::new(level.clone()),
            your_team,
            players: players.to_vec(),
        });
    }

    pub fn accept_offer(&self) {
        self.send(&Message::Ready);
    }

    pub fn go(&self) {
        self.send(&Message::Go);
    }

    /// Sends a chat line in `team`'s name; it is cleaned first, and `None` when nothing printable is left.
    pub fn say(&self, team: Team, text: &str) -> Option<String> {
        let text = chat_line(text)?;
        self.send(&Message::Chat {
            team,
            text: text.clone(),
        });
        Some(text)
    }

    /// Passes a chat line from one guest on to another.
    pub fn relay_chat(&self, team: Team, text: &str) {
        self.send(&Message::Chat {
            team,
            text: text.to_string(),
        });
    }

    /// Lobby messages since the last call; battle messages that arrive early are kept for the battle.
    pub fn lobby_news(&mut self) -> Vec<LobbyNews> {
        let mut news = Vec::new();
        for arrival in self.drain() {
            match arrival {
                Arrival::Message(Message::Offer {
                    version,
                    level,
                    your_team,
                    players,
                }) => news.push(LobbyNews::Offered(
                    if version == env!("CARGO_PKG_VERSION") {
                        Ok(Box::new(Offer {
                            level: *level,
                            your_team,
                            players,
                        }))
                    } else {
                        Err(version)
                    },
                )),
                Arrival::Message(Message::Ready) => news.push(LobbyNews::GuestReady),
                Arrival::Message(Message::Go) => news.push(LobbyNews::Go),
                Arrival::Message(Message::Ping { nonce }) => {
                    self.send(&Message::Pong { nonce });
                }
                Arrival::Message(Message::Chat { team, text }) => {
                    news.extend(chat_line(&text).map(|text| LobbyNews::Chat(team, text)));
                }
                Arrival::ConnectionLost => news.push(LobbyNews::Left),
                battle => self.early.push(battle),
            }
        }
        news
    }
}

/// The reading thread holds its own handle on the connection, so leaving must close it outright for the
/// other machine to notice.
impl Drop for Link {
    fn drop(&mut self) {
        if let Ok(stream) = self.writer.lock() {
            let _ = stream.shutdown(std::net::Shutdown::Both);
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SessionState {
    InStep,
    /// The machines computed different battles at this tick; play cannot continue.
    Desynced(u64),
    Disconnected,
}

/// A battle with players on other machines: every machine runs the whole simulation and they exchange only
/// commands. A guest has one link, to the host; the host has one per guest and passes messages between them.
#[derive(Resource)]
pub struct NetSession {
    links: Vec<Link>,
    relays: bool,
    remote_teams: Vec<Team>,
    /// This machine only watches: it sends no orders and no fingerprints.
    spectating: bool,
    /// Links to spectators; losing one does not stop the battle.
    watcher_links: BTreeSet<usize>,
    closed_links: BTreeSet<usize>,
    remote_turns: BTreeSet<(u64, usize)>,
    local_checksums: BTreeMap<u64, u64>,
    remote_checksums: BTreeMap<(u64, usize), u64>,
    pub state: SessionState,
    reported: Option<SessionState>,
    pings_in_flight: BTreeMap<u64, Instant>,
    incoming_chat: Vec<(Team, String)>,
    next_ping: u64,
    last_ping_sent: Option<Instant>,
    round_trip: Option<Duration>,
}

impl NetSession {
    /// `relays` is true for the host, which passes each guest's messages on to the other guests.
    pub fn new(links: Vec<Link>, remote_teams: Vec<Team>, relays: bool) -> Self {
        Self {
            links,
            relays,
            remote_teams,
            spectating: false,
            watcher_links: BTreeSet::new(),
            closed_links: BTreeSet::new(),
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
        }
    }

    /// A battle against one other machine over `stream`.
    pub fn with_one_player(stream: TcpStream, remote_team: Team) -> std::io::Result<Self> {
        Ok(Self::new(
            vec![Link::new(stream)?],
            vec![remote_team],
            false,
        ))
    }

    /// A spectator's session: it follows the orders of every team in `players` and gives none.
    pub fn watching(links: Vec<Link>, players: Vec<Team>) -> Self {
        Self {
            spectating: true,
            ..Self::new(links, players, false)
        }
    }

    /// Marks the links at `indices` as spectators' on the host.
    pub fn with_watchers(mut self, indices: impl IntoIterator<Item = usize>) -> Self {
        self.watcher_links.extend(indices);
        self
    }

    pub fn is_spectating(&self) -> bool {
        self.spectating
    }

    fn lose_link(&mut self, index: usize) {
        if self.watcher_links.contains(&index) {
            self.closed_links.insert(index);
        } else {
            self.state = SessionState::Disconnected;
        }
    }

    pub fn remote_teams(&self) -> &[Team] {
        &self.remote_teams
    }

    /// Sends a chat line in `team`'s name to every other player; returns the cleaned line.
    pub fn say(&mut self, team: Team, text: &str) -> Option<String> {
        let text = chat_line(text)?;
        self.broadcast(&Message::Chat {
            team,
            text: text.clone(),
        });
        Some(text)
    }

    /// Chat lines other players sent since the last call, with the team that sent each.
    pub fn take_chat(&mut self) -> Vec<(Team, String)> {
        std::mem::take(&mut self.incoming_chat)
    }

    /// The smoothed time a message takes to another machine and back; `None` until first measured.
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
        self.broadcast(&Message::Ping { nonce });
    }

    fn answered(&mut self, nonce: u64) {
        let Some(&sent) = self.pings_in_flight.get(&nonce) else {
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

    /// Whether every other player's commands for `tick` have arrived; the first ticks need none.
    fn has_turns_for(&self, tick: u64) -> bool {
        tick < NETWORK_COMMAND_DELAY_TICKS
            || self
                .remote_teams
                .iter()
                .all(|team| self.remote_turns.contains(&(tick, team.index())))
    }

    fn broadcast(&mut self, message: &Message) {
        self.send_except(message, None);
    }

    fn send_except(&mut self, message: &Message, skipped: Option<usize>) {
        let lost: Vec<usize> = self
            .links
            .iter()
            .enumerate()
            .filter(|(index, _)| Some(*index) != skipped && !self.closed_links.contains(index))
            .filter(|(_, link)| !link.send(message))
            .map(|(index, _)| index)
            .collect();
        for index in lost {
            self.lose_link(index);
        }
    }

    /// Once both fingerprints of a tick are known they are compared; the remote one is then forgotten.
    fn compare_checksums(&mut self, tick: u64, team: usize) {
        let (Some(&local), Some(&remote)) = (
            self.local_checksums.get(&tick),
            self.remote_checksums.get(&(tick, team)),
        ) else {
            return;
        };
        self.remote_checksums.remove(&(tick, team));
        if local != remote && self.state == SessionState::InStep {
            self.state = SessionState::Desynced(tick);
        }
    }

    /// Reads every link; the host passes what a guest says about the battle on to the other guests.
    fn receive(&mut self, queue: &mut CommandQueue) {
        for index in 0..self.links.len() {
            for arrival in self.links[index].drain() {
                let message = match arrival {
                    Arrival::Message(message) => message,
                    Arrival::ConnectionLost => {
                        self.lose_link(index);
                        continue;
                    }
                };
                let shared = matches!(
                    message,
                    Message::Turn { .. } | Message::Checksum { .. } | Message::Chat { .. }
                );
                if self.relays && shared {
                    self.send_except(&message, Some(index));
                }
                match message {
                    Message::Turn {
                        team,
                        tick,
                        commands,
                    } => {
                        for command in commands {
                            queue.schedule(ScheduledCommand {
                                tick,
                                team,
                                command,
                            });
                        }
                        self.remote_turns.insert((tick, team.index()));
                    }
                    Message::Checksum { team, tick, sum } => {
                        self.remote_checksums.insert((tick, team.index()), sum);
                        self.compare_checksums(tick, team.index());
                    }
                    Message::Chat { team, text } => {
                        self.incoming_chat
                            .extend(chat_line(&text).map(|text| (team, text)));
                    }
                    Message::Ping { nonce } => {
                        self.links[index].send(&Message::Pong { nonce });
                    }
                    Message::Pong { nonce } => self.answered(nonce),
                    Message::Offer { .. } | Message::Ready | Message::Go => {}
                }
            }
        }
    }
}

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

/// The simulation may take its next step only once every other player's commands for it are known.
pub fn simulation_may_advance(session: Option<Res<NetSession>>, tick: Res<SimulationTick>) -> bool {
    session.is_none_or(|session| {
        session.state == SessionState::InStep && session.has_turns_for(tick.0)
    })
}

pub fn receive_turns(session: Option<ResMut<NetSession>>, mut queue: ResMut<CommandQueue>) {
    if let Some(mut session) = session {
        session.receive(&mut queue);
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
    session
        .remote_turns
        .retain(|&(received, _)| received >= tick.0);
    if session.spectating {
        return;
    }
    let turn = tick.0 + NETWORK_COMMAND_DELAY_TICKS;
    let message = Message::Turn {
        team: local.0,
        tick: turn,
        commands: queue.scheduled_for(turn, local.0),
    };
    session.broadcast(&message);
}

pub fn fingerprint_tick(world: &mut World) {
    let tick = world.resource::<SimulationTick>().0;
    if !world.contains_resource::<NetSession>() || !tick.is_multiple_of(CHECKSUM_EVERY_TICKS) {
        return;
    }
    let sum = simulation_checksum(world);
    let team = world.resource::<LocalTeam>().0;
    let mut session = world.resource_mut::<NetSession>();
    session.local_checksums.insert(tick, sum);
    session
        .local_checksums
        .retain(|&checked, _| checked + CHECKSUM_EVERY_TICKS * 30 > tick);
    if !session.spectating {
        session.broadcast(&Message::Checksum { team, tick, sum });
    }
    let teams: Vec<usize> = session
        .remote_teams
        .iter()
        .map(|team| team.index())
        .collect();
    for team in teams {
        session.compare_checksums(tick, team);
    }
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
        SessionState::Disconnected => tr("Another player has left the battle").to_string(),
        SessionState::Desynced(tick) => {
            tr_fmt("Out of step with the other players at tick {}", &[&tick])
        }
        SessionState::InStep => return,
    });
    session.reported = Some(session.state);
}

/// Teams played by people on any machine get no computer commander.
pub fn retire_commanders_of_players(
    session: Option<Res<NetSession>>,
    local: Res<LocalTeam>,
    mut commanders: ResMut<AiCommanders>,
) {
    let Some(session) = session else { return };
    commanders.0.retain(|commander| {
        commander.team != local.0 && !session.remote_teams.contains(&commander.team)
    });
}
