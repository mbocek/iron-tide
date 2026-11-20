use std::collections::{BTreeMap, BTreeSet};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use tungstenite::protocol::{Role, WebSocketConfig};
use tungstenite::{Message as WsMessage, WebSocket};

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
/// A late spectator's catch-up carries every order of the battle so far.
const LARGEST_MESSAGE_BYTES: usize = 1 << 24;
/// At most this many people watch one battle.
pub const MOST_SPECTATORS: usize = 4;
/// A spectator further behind than this (two seconds) runs the battle fast until it has caught up.
const CATCH_UP_FROM_TICKS: u64 = 40;
const CATCH_UP_SPEED: f32 = 40.0;
/// How long the host holds a battle for a player whose connection dropped before giving up on them.
const REJOIN_WINDOW: Duration = Duration::from_secs(180);
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
        /// The battle has already begun: the guest can only watch, after catching up.
        #[serde(default)]
        under_way: bool,
    },
    /// Every order of a battle under way, for a spectator who joins late: each team's turns are complete
    /// through its tick in `through`, and only turns with orders are listed.
    CatchUp {
        through: Vec<(Team, u64)>,
        turns: Vec<(Team, u64, Vec<PlayerCommand>)>,
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
    pub under_way: bool,
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

/// Which end of the WebSocket handshake this machine plays: the host answers it, a guest opens it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Side {
    Host,
    Guest,
}

/// Where outgoing messages go: queued until the handshake on the link's thread has finished.
#[derive(Default)]
struct Outbox {
    socket: Option<WebSocket<TcpStream>>,
    waiting: Vec<Vec<u8>>,
    failed: bool,
}

/// One WebSocket connection to another machine, so a dedicated server on the internet can later take the
/// host's place. Each message is one binary frame of JSON; the handshake and all reading happen on the
/// link's own thread.
pub struct Link {
    outbox: Arc<Mutex<Outbox>>,
    /// The underlying socket, to close it outright when the link is dropped.
    raw: TcpStream,
    inbox: Mutex<Receiver<Arrival>>,
    /// Battle messages that arrived while the lobby was still being set up.
    early: Vec<Arrival>,
}

fn socket_config() -> WebSocketConfig {
    WebSocketConfig::default()
        .max_message_size(Some(LARGEST_MESSAGE_BYTES))
        .max_frame_size(Some(LARGEST_MESSAGE_BYTES))
}

fn handshake(stream: TcpStream, side: Side) -> Option<WebSocket<TcpStream>> {
    match side {
        Side::Host => tungstenite::accept_with_config(stream, Some(socket_config())).ok(),
        Side::Guest => {
            let address = stream.peer_addr().ok()?;
            tungstenite::client::client_with_config(
                format!("ws://{address}/"),
                stream,
                Some(socket_config()),
            )
            .ok()
            .map(|(socket, _)| socket)
        }
    }
}

fn read_arrival(socket: &mut WebSocket<TcpStream>) -> Option<Arrival> {
    match socket.read() {
        Ok(WsMessage::Binary(bytes)) => {
            Some(serde_json::from_slice(&bytes).map_or(Arrival::ConnectionLost, Arrival::Message))
        }
        Ok(WsMessage::Close(_)) | Err(_) => Some(Arrival::ConnectionLost),
        Ok(_) => None,
    }
}

impl Link {
    pub fn host_side(stream: TcpStream) -> std::io::Result<Self> {
        Self::open(stream, Side::Host)
    }

    pub fn guest_side(stream: TcpStream) -> std::io::Result<Self> {
        Self::open(stream, Side::Guest)
    }

    pub fn open(stream: TcpStream, side: Side) -> std::io::Result<Self> {
        stream.set_nodelay(true)?;
        let raw = stream.try_clone()?;
        let for_writing = stream.try_clone()?;
        let outbox = Arc::new(Mutex::new(Outbox::default()));
        let (sender, receiver) = channel();
        let shared_outbox = Arc::clone(&outbox);
        std::thread::spawn(move || {
            let Some(mut reader) = handshake(stream, side) else {
                if let Ok(mut outbox) = shared_outbox.lock() {
                    outbox.failed = true;
                }
                let _ = sender.send(Arrival::ConnectionLost);
                return;
            };
            let role = match side {
                Side::Host => Role::Server,
                Side::Guest => Role::Client,
            };
            // Writing from a second handle is safe because the reading one only writes when it answers a
            // ping or a close, and these peers send neither while the battle runs.
            let mut writer = WebSocket::from_raw_socket(for_writing, role, Some(socket_config()));
            if let Ok(mut outbox) = shared_outbox.lock() {
                for bytes in std::mem::take(&mut outbox.waiting) {
                    outbox.failed |= writer.send(WsMessage::Binary(bytes.into())).is_err();
                }
                outbox.socket = Some(writer);
            }
            loop {
                let Some(arrival) = read_arrival(&mut reader) else {
                    continue;
                };
                let lost = matches!(arrival, Arrival::ConnectionLost);
                if sender.send(arrival).is_err() || lost {
                    return;
                }
            }
        });
        Ok(Self {
            outbox,
            raw,
            inbox: Mutex::new(receiver),
            early: Vec::new(),
        })
    }

    /// Returns whether the message left or waits for the handshake; a failure means the connection is gone.
    fn send(&self, message: &Message) -> bool {
        let Ok(bytes) = serde_json::to_vec(message) else {
            return true;
        };
        let Ok(mut outbox) = self.outbox.lock() else {
            return false;
        };
        if outbox.failed {
            return false;
        }
        match outbox.socket.as_mut() {
            Some(socket) => {
                let sent = socket.send(WsMessage::Binary(bytes.into())).is_ok();
                outbox.failed = !sent;
                sent
            }
            None => {
                outbox.waiting.push(bytes);
                true
            }
        }
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
            under_way: false,
        });
    }

    /// Offers a battle under way: to watch, or to take back the seat of a player who lost the connection.
    fn offer_under_way(&self, level: &LevelDefinition, your_team: Option<Team>, players: &[Team]) {
        self.send(&Message::Offer {
            version: env!("CARGO_PKG_VERSION").to_string(),
            level: Box::new(level.clone()),
            your_team,
            players: players.to_vec(),
            under_way: true,
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
                    under_way,
                }) => news.push(LobbyNews::Offered(
                    if version == env!("CARGO_PKG_VERSION") {
                        Ok(Box::new(Offer {
                            level: *level,
                            your_team,
                            players,
                            under_way,
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
        let _ = self.raw.shutdown(std::net::Shutdown::Both);
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
    /// On the host: where late spectators knock, and what they need to catch up.
    door: Option<OpenDoor>,
    /// On the host, which link carries which other player's orders.
    link_teams: BTreeMap<usize, Team>,
    /// On the host, players whose connection dropped, by team index, and since when the battle waits for them.
    away: BTreeMap<usize, (Team, Instant)>,
    /// On a machine that joined late, the latest tick of each team's orders the host already had; a returning
    /// player must not send those turns again.
    turns_known_through: BTreeMap<usize, u64>,
}

/// The host keeps accepting spectators during the battle and remembers every order to bring them up to date.
struct OpenDoor {
    listener: TcpListener,
    level: LevelDefinition,
    players: Vec<Team>,
    /// Late spectators considering the offer, not yet watching.
    newcomers: Vec<(Link, Option<Team>)>,
    history: TurnHistory,
}

#[derive(Default)]
struct TurnHistory {
    /// Keyed by team index; the latest tick of each team's turns.
    through: BTreeMap<usize, (Team, u64)>,
    turns: Vec<(Team, u64, Vec<PlayerCommand>)>,
}

impl TurnHistory {
    /// Each team's turns reach the host in tick order, so the latest one closes every gap before it.
    fn record(&mut self, team: Team, tick: u64, commands: &[PlayerCommand]) {
        let latest = self.through.entry(team.index()).or_insert((team, tick));
        latest.1 = latest.1.max(tick);
        if !commands.is_empty() {
            self.turns.push((team, tick, commands.to_vec()));
        }
    }

    fn catch_up(&self) -> Message {
        Message::CatchUp {
            through: self.through.values().copied().collect(),
            turns: self.turns.clone(),
        }
    }
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
            door: None,
            link_teams: BTreeMap::new(),
            away: BTreeMap::new(),
            turns_known_through: BTreeMap::new(),
        }
    }

    /// Keeps `listener` open during the battle so spectators can still join, offered `level`.
    pub fn with_open_door(mut self, listener: TcpListener, level: LevelDefinition) -> Self {
        let players = std::iter::once(Team::Player)
            .chain(self.remote_teams.iter().copied())
            .collect();
        let player_links =
            (0..self.links.len()).filter(|index| !self.watcher_links.contains(index));
        self.link_teams = player_links
            .zip(self.remote_teams.iter().copied())
            .collect();
        self.door = Some(OpenDoor {
            listener,
            level,
            players,
            newcomers: Vec::new(),
            history: TurnHistory::default(),
        });
        self
    }

    fn remember_turn(&mut self, team: Team, tick: u64, commands: &[PlayerCommand]) {
        if let Some(door) = &mut self.door {
            door.history.record(team, tick, commands);
        }
    }

    /// The last tick for which every other player's orders are known; a spectator far behind it catches up.
    pub fn known_through(&self) -> Option<u64> {
        self.remote_teams
            .iter()
            .map(|team| {
                self.remote_turns
                    .iter()
                    .filter(|(_, index)| *index == team.index())
                    .map(|(tick, _)| *tick)
                    .max()
            })
            .min()
            .flatten()
    }

    /// Lets late guests in: a player whose connection dropped gets their seat back, anyone else may watch.
    /// Once they accept they get every order so far.
    fn admit_late_guests(&mut self) {
        if self
            .away
            .values()
            .any(|(_, since)| since.elapsed() > REJOIN_WINDOW)
        {
            self.state = SessionState::Disconnected;
        }
        let watching =
            self.watcher_links.len() - self.closed_links.intersection(&self.watcher_links).count();
        let Some(door) = &mut self.door else { return };
        let free_seat = self.away.values().map(|&(team, _)| team).find(|&team| {
            door.newcomers
                .iter()
                .all(|(_, offered)| *offered != Some(team))
        });
        let spectator_room = watching + door.newcomers.len() < MOST_SPECTATORS;
        if (free_seat.is_some() || spectator_room)
            && let Ok((stream, _)) = door.listener.accept()
            && stream.set_nonblocking(false).is_ok()
            && let Ok(link) = Link::host_side(stream)
        {
            link.offer_under_way(&door.level, free_seat, &door.players);
            door.newcomers.push((link, free_seat));
        }
        let mut admitted = Vec::new();
        let mut index = 0;
        while index < door.newcomers.len() {
            let news = door.newcomers[index].0.lobby_news();
            if news.contains(&LobbyNews::Left) {
                door.newcomers.remove(index);
            } else if news.contains(&LobbyNews::GuestReady) {
                let (link, seat) = door.newcomers.remove(index);
                link.send(&door.history.catch_up());
                link.go();
                admitted.push((link, seat));
            } else {
                index += 1;
            }
        }
        for (link, seat) in admitted {
            let index = self.links.len();
            match seat {
                Some(team) => {
                    self.link_teams.insert(index, team);
                    self.away.remove(&team.index());
                }
                None => {
                    self.watcher_links.insert(index);
                }
            }
            self.links.push(link);
        }
    }

    /// On the host: a player who lost the connection and how long the battle has waited for them.
    pub fn waiting_for_rejoin(&self) -> Option<(Team, Duration)> {
        self.away
            .values()
            .map(|&(team, since)| (team, since.elapsed()))
            .max_by_key(|&(_, waited)| waited)
    }

    /// A battle against one other machine over `stream`.
    pub fn with_one_player(
        stream: TcpStream,
        side: Side,
        remote_team: Team,
    ) -> std::io::Result<Self> {
        Ok(Self::new(
            vec![Link::open(stream, side)?],
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

    /// A spectator may go; a player's dropped connection holds the battle for a while on a host that can
    /// take them back, and ends it anywhere else.
    fn lose_link(&mut self, index: usize) {
        if self.watcher_links.contains(&index) {
            self.closed_links.insert(index);
            return;
        }
        match self
            .link_teams
            .remove(&index)
            .filter(|_| self.door.is_some())
        {
            Some(team) => {
                self.closed_links.insert(index);
                self.away.insert(team.index(), (team, Instant::now()));
            }
            None => self.state = SessionState::Disconnected,
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
                        self.remember_turn(team, tick, &commands);
                        for command in commands {
                            queue.schedule(ScheduledCommand {
                                tick,
                                team,
                                command,
                            });
                        }
                        self.remote_turns.insert((tick, team.index()));
                    }
                    Message::CatchUp { through, turns } => {
                        for (team, last) in through {
                            self.remote_turns
                                .extend((0..=last).map(|tick| (tick, team.index())));
                            self.turns_known_through.insert(team.index(), last);
                        }
                        for (team, tick, commands) in turns {
                            for command in commands {
                                queue.schedule(ScheduledCommand {
                                    tick,
                                    team,
                                    command,
                                });
                            }
                        }
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
    let already_sent = session
        .turns_known_through
        .get(&local.0.index())
        .is_some_and(|&known| turn <= known);
    if already_sent {
        return;
    }
    let commands = queue.scheduled_for(turn, local.0);
    session.remember_turn(local.0, turn, &commands);
    let message = Message::Turn {
        team: local.0,
        tick: turn,
        commands,
    };
    session.broadcast(&message);
}

pub fn admit_late_guests(session: Option<ResMut<NetSession>>) {
    if let Some(mut session) = session {
        session.admit_late_guests();
    }
}

/// How far a late spectator still has to run to reach the battle as it stands, as a share done; `None` when
/// it is caught up.
#[derive(Resource, Default)]
pub struct CatchingUp(pub Option<f32>);

/// A machine that joined late runs the battle many times faster, silently, until it reaches the players.
pub fn catch_up_with_the_battle(
    session: Option<Res<NetSession>>,
    tick: Res<SimulationTick>,
    settings: Res<crate::settings::GameSettings>,
    mut catching_up: ResMut<CatchingUp>,
    mut time: ResMut<Time<Virtual>>,
    mut volume: ResMut<GlobalVolume>,
) {
    let behind = session
        .and_then(|session| session.known_through())
        .map(|known| (known.saturating_sub(tick.0), known));
    let far_behind = behind.filter(|&(ticks, _)| ticks > CATCH_UP_FROM_TICKS);
    match (far_behind, catching_up.0.is_some()) {
        (Some((ticks, known)), _) => {
            catching_up.0 = Some(1.0 - ticks as f32 / known.max(1) as f32);
            time.set_relative_speed(CATCH_UP_SPEED);
            volume.volume = bevy::audio::Volume::SILENT;
        }
        (None, true) => {
            catching_up.0 = None;
            time.set_relative_speed(settings.game_speed_factor());
            volume.volume = crate::sound::master_volume(&settings);
        }
        (None, false) => {}
    }
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
