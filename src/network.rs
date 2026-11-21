use std::collections::{BTreeMap, BTreeSet};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use tungstenite::protocol::{Role, WebSocketConfig};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Connector, Message as WsMessage, WebSocket};

use crate::ai::AiCommanders;
use crate::catalog::Team;
use crate::determinism::simulation_checksum;
use crate::i18n::{tr, tr_fmt};
use crate::levels::LevelDefinition;
use crate::notifications::Notifications;
use crate::player_commands::{
    CommandQueue, LocalTeam, PlayerCommand, ScheduledCommand, SimulationTick,
};
use crate::relay::Relay;

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
/// Bevy clamps each frame's scaled game time to `max_delta`, 250 ms by default, which would hold catching up
/// to 5 ticks a frame whatever the speed.
const CATCH_UP_LONGEST_FRAME: Duration = Duration::from_secs(2);
const REGULAR_LONGEST_FRAME: Duration = Duration::from_millis(250);
const PING_INTERVAL: Duration = Duration::from_secs(1);
const PINGS_REMEMBERED: u64 = 10;
const ROUND_TRIP_SMOOTHING: f32 = 0.3;
const LONGEST_CHAT_LINE: usize = 120;

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub enum Message {
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
    /// To a dedicated server: the first player in a room sets its battle for `players` people in all.
    OpenRoom {
        version: String,
        level: Box<LevelDefinition>,
        players: usize,
    },
    /// From a dedicated server to the player who opened the room: how many guests have a seat and are ready.
    RoomStatus {
        seated: usize,
        ready: usize,
    },
    /// To a dedicated server: the player who opened the room starts the battle.
    Start,
    /// The other end will not have this connection, and why.
    Refused {
        reason: String,
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
    /// From a dedicated server, to the player who opened the room: guests seated and ready.
    RoomStatus {
        seated: usize,
        ready: usize,
    },
    Refused(String),
    Left,
}

pub enum Arrival {
    Message(Message),
    ConnectionLost,
}

/// Which end of the WebSocket handshake this machine plays: the host answers it, a guest opens it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Side {
    Host,
    Guest,
}

/// Where a guest connects: the address to dial and the WebSocket URL to ask for there.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct JoinTarget {
    pub address: String,
    pub url: String,
}

/// A server behind a TLS proxy listens where web browsers expect it.
const SECURE_PORT: u16 = 443;

impl JoinTarget {
    /// Accepts `host`, `host:port`, `ws://host[:port]/path` or `wss://host[:port]/path`; a path names a room
    /// on a dedicated server, and a host in the game ignores it.
    pub fn parse(text: &str, default_port: u16) -> Result<JoinTarget, String> {
        let text = text.trim();
        let (scheme, rest, port) = match text.strip_prefix("wss://") {
            Some(rest) => ("wss", rest, SECURE_PORT),
            None => (
                "ws",
                text.strip_prefix("ws://").unwrap_or(text),
                default_port,
            ),
        };
        let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
        if host.is_empty() {
            return Err(tr("Type the address of the host").to_string());
        }
        let address = if host.contains(':') {
            host.to_string()
        } else {
            format!("{host}:{port}")
        };
        let url = format!("{scheme}://{address}/{path}");
        Ok(JoinTarget { address, url })
    }
}

type Socket = WebSocket<MaybeTlsStream<TcpStream>>;

/// One WebSocket connection to another machine, plain or over TLS. Each message is one binary frame of JSON,
/// queued by `send` for the connection's own threads.
pub struct Link {
    outbox: Sender<Vec<u8>>,
    /// The connection's thread has ended: the handshake failed or the connection went.
    failed: Arc<AtomicBool>,
    /// The handshake is done; from then on dropping the link sends what is queued before closing.
    serving: Arc<AtomicBool>,
    /// On the answering side, the path the other end asked for, once its handshake has arrived.
    requested_path: Arc<Mutex<Option<String>>>,
    /// The underlying socket, to close it outright when the link is dropped.
    raw: TcpStream,
    inbox: Mutex<Receiver<Arrival>>,
    /// Battle messages that arrived while the lobby was still being set up.
    early: Vec<Arrival>,
}

/// How a guest's link opens: the URL to ask for, and for `wss://` the certificates to trust; `None` trusts
/// the public web's.
pub struct Dialling {
    pub url: String,
    pub trusted: Option<Arc<rustls::RootCertStore>>,
}

/// How often a TLS connection's thread stops waiting for messages to send queued ones.
const SOCKET_POLL: Duration = Duration::from_millis(5);

fn socket_config() -> WebSocketConfig {
    WebSocketConfig::default()
        .max_message_size(Some(LARGEST_MESSAGE_BYTES))
        .max_frame_size(Some(LARGEST_MESSAGE_BYTES))
}

/// The certificates of the public web, which a dedicated server behind an ordinary TLS proxy presents.
fn public_roots() -> Arc<rustls::RootCertStore> {
    Arc::new(rustls::RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    })
}

fn tls_connector(trusted: Arc<rustls::RootCertStore>) -> Option<Connector> {
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .ok()?
    .with_root_certificates(trusted)
    .with_no_client_auth();
    Some(Connector::Rustls(Arc::new(config)))
}

// tungstenite's handshake callback must return its own `ErrorResponse`, however large clippy finds it.
#[allow(clippy::result_large_err)]
fn handshake(
    stream: TcpStream,
    side: Side,
    dialling: Option<Dialling>,
    requested_path: &Mutex<Option<String>>,
) -> Option<Socket> {
    match side {
        Side::Host => {
            let remember_path =
                |request: &tungstenite::handshake::server::Request,
                 response: tungstenite::handshake::server::Response| {
                    if let Ok(mut path) = requested_path.lock() {
                        *path = Some(request.uri().path().to_string());
                    }
                    Ok(response)
                };
            tungstenite::accept_hdr_with_config(
                MaybeTlsStream::Plain(stream),
                remember_path,
                Some(socket_config()),
            )
            .ok()
        }
        Side::Guest => {
            let Dialling { url, trusted } = match dialling {
                Some(dialling) => dialling,
                None => Dialling {
                    url: format!("ws://{}/", stream.peer_addr().ok()?),
                    trusted: None,
                },
            };
            let connector = if url.starts_with("wss://") {
                tls_connector(trusted.unwrap_or_else(public_roots))?
            } else {
                Connector::Plain
            };
            tungstenite::client_tls_with_config(url, stream, Some(socket_config()), Some(connector))
                .ok()
                .map(|(socket, _)| socket)
        }
    }
}

fn is_waiting(error: &tungstenite::Error) -> bool {
    matches!(error, tungstenite::Error::Io(io)
        if matches!(io.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut))
}

fn read_arrival(socket: &mut Socket) -> Option<Arrival> {
    match socket.read() {
        Ok(WsMessage::Binary(bytes)) => {
            Some(serde_json::from_slice(&bytes).map_or(Arrival::ConnectionLost, Arrival::Message))
        }
        Ok(WsMessage::Close(_)) => Some(Arrival::ConnectionLost),
        Ok(_) => None,
        Err(error) if is_waiting(&error) => None,
        Err(_) => Some(Arrival::ConnectionLost),
    }
}

/// A plain connection is read on this thread and written on another, each through its own handle, so a
/// queued message leaves at once. Writing from a second handle is safe because the reading one only writes
/// when it answers a ping or a close, and these peers send neither while the battle runs.
fn serve_split(
    mut socket: Socket,
    writer: TcpStream,
    side: Side,
    outbox: Receiver<Vec<u8>>,
    inbox: &Sender<Arrival>,
) {
    let role = match side {
        Side::Host => Role::Server,
        Side::Guest => Role::Client,
    };
    let mut writer =
        WebSocket::from_raw_socket(MaybeTlsStream::Plain(writer), role, Some(socket_config()));
    std::thread::spawn(move || {
        for bytes in outbox {
            if writer.send(WsMessage::Binary(bytes.into())).is_err() {
                return;
            }
        }
        if let MaybeTlsStream::Plain(stream) = writer.get_mut() {
            let _ = stream.shutdown(std::net::Shutdown::Both);
        }
    });
    loop {
        let Some(arrival) = read_arrival(&mut socket) else {
            continue;
        };
        let lost = matches!(arrival, Arrival::ConnectionLost);
        if inbox.send(arrival).is_err() || lost {
            return;
        }
    }
}

/// A TLS connection keeps its state in one place, so one thread owns it: it reads with a short timeout and
/// sends what was queued in between, at the cost of up to `SOCKET_POLL` before a message leaves.
fn serve_polling(mut socket: Socket, outbox: &Receiver<Vec<u8>>, inbox: &Sender<Arrival>) {
    loop {
        loop {
            match outbox.try_recv() {
                Ok(bytes) => {
                    if socket.send(WsMessage::Binary(bytes.into())).is_err() {
                        return;
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    let _ = socket.close(None);
                    let _ = socket.flush();
                    return;
                }
            }
        }
        let arrival = match socket.read() {
            Ok(WsMessage::Binary(bytes)) => {
                serde_json::from_slice(&bytes).map_or(Arrival::ConnectionLost, Arrival::Message)
            }
            Ok(WsMessage::Close(_)) => return,
            Ok(_) => continue,
            Err(error) if is_waiting(&error) => continue,
            Err(_) => return,
        };
        let lost = matches!(arrival, Arrival::ConnectionLost);
        if inbox.send(arrival).is_err() || lost {
            return;
        }
    }
}

impl Link {
    pub fn host_side(stream: TcpStream) -> std::io::Result<Self> {
        Self::open_with(stream, Side::Host, None)
    }

    pub fn guest_side(stream: TcpStream) -> std::io::Result<Self> {
        Self::open_with(stream, Side::Guest, None)
    }

    /// A guest's link that asks the other end for `dialling.url`, which may name a room and may be `wss://`.
    pub fn guest_side_to(stream: TcpStream, dialling: Dialling) -> std::io::Result<Self> {
        Self::open_with(stream, Side::Guest, Some(dialling))
    }

    pub fn open(stream: TcpStream, side: Side) -> std::io::Result<Self> {
        Self::open_with(stream, side, None)
    }

    fn open_with(
        stream: TcpStream,
        side: Side,
        dialling: Option<Dialling>,
    ) -> std::io::Result<Self> {
        stream.set_nodelay(true)?;
        let raw = stream.try_clone()?;
        let second_handle = stream.try_clone()?;
        let failed = Arc::new(AtomicBool::new(false));
        let serving = Arc::new(AtomicBool::new(false));
        let requested_path = Arc::new(Mutex::new(None));
        let (outbox, queued) = channel::<Vec<u8>>();
        let (arrived, inbox) = channel();
        let thread_failed = Arc::clone(&failed);
        let thread_serving = Arc::clone(&serving);
        let thread_path = Arc::clone(&requested_path);
        std::thread::spawn(move || {
            if let Some(socket) = handshake(stream, side, dialling, &thread_path) {
                thread_serving.store(true, Ordering::Relaxed);
                let encrypted = matches!(socket.get_ref(), MaybeTlsStream::Rustls(_));
                if !encrypted {
                    serve_split(socket, second_handle, side, queued, &arrived);
                } else if second_handle.set_read_timeout(Some(SOCKET_POLL)).is_ok() {
                    serve_polling(socket, &queued, &arrived);
                }
            }
            thread_failed.store(true, Ordering::Relaxed);
            let _ = arrived.send(Arrival::ConnectionLost);
        });
        Ok(Self {
            outbox,
            failed,
            serving,
            requested_path,
            raw,
            inbox: Mutex::new(inbox),
            early: Vec::new(),
        })
    }

    /// The path the other end asked for in its handshake, e.g. `/room/K7Q2`; `None` until it has arrived.
    pub fn requested_path(&self) -> Option<String> {
        self.requested_path
            .lock()
            .ok()
            .and_then(|path| path.clone())
    }

    /// Whether the connection failed before or after its handshake.
    pub fn has_failed(&self) -> bool {
        self.failed.load(Ordering::Relaxed)
    }

    /// Queues the message for the connection's thread; false once the connection has gone.
    pub fn send(&self, message: &Message) -> bool {
        let Ok(bytes) = serde_json::to_vec(message) else {
            return true;
        };
        !self.has_failed() && self.outbox.send(bytes).is_ok()
    }

    pub fn drain(&mut self) -> Vec<Arrival> {
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
    pub fn offer_under_way(
        &self,
        level: &LevelDefinition,
        your_team: Option<Team>,
        players: &[Team],
    ) {
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

    /// Sets the battle of the room this link leads to on a dedicated server.
    pub fn open_room(&self, level: &LevelDefinition, players: usize) {
        self.send(&Message::OpenRoom {
            version: env!("CARGO_PKG_VERSION").to_string(),
            level: Box::new(level.clone()),
            players,
        });
    }

    pub fn start_room(&self) {
        self.send(&Message::Start);
    }

    pub fn refuse(&self, reason: &str) {
        self.send(&Message::Refused {
            reason: reason.to_string(),
        });
    }

    pub fn report_room(&self, seated: usize, ready: usize) {
        self.send(&Message::RoomStatus { seated, ready });
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
                Arrival::Message(Message::RoomStatus { seated, ready }) => {
                    news.push(LobbyNews::RoomStatus { seated, ready });
                }
                Arrival::Message(Message::Refused { reason }) => {
                    news.push(LobbyNews::Refused(reason))
                }
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

/// Once connected, dropping the link lets its threads send what is still queued and then close the
/// connection, so a last message such as a refusal arrives; a link still in its handshake closes at once.
impl Drop for Link {
    fn drop(&mut self) {
        if !self.serving.load(Ordering::Relaxed) {
            let _ = self.raw.shutdown(std::net::Shutdown::Both);
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

/// Which ticks' turns have arrived from one team, kept as the tick every turn up to which is known plus the few
/// that arrived ahead of a gap.
struct ReceivedTurns {
    all_through: u64,
    ahead: BTreeSet<u64>,
}

impl Default for ReceivedTurns {
    fn default() -> Self {
        Self {
            all_through: NETWORK_COMMAND_DELAY_TICKS - 1,
            ahead: BTreeSet::new(),
        }
    }
}

impl ReceivedTurns {
    fn has(&self, tick: u64) -> bool {
        tick <= self.all_through || self.ahead.contains(&tick)
    }

    fn latest(&self) -> u64 {
        self.ahead.last().copied().unwrap_or(self.all_through)
    }

    fn record(&mut self, tick: u64) {
        if !self.has(tick) {
            self.ahead.insert(tick);
            self.close_gaps();
        }
    }

    fn record_all_through(&mut self, last: u64) {
        self.all_through = self.all_through.max(last);
        self.ahead.retain(|&tick| tick > last);
        self.close_gaps();
    }

    fn close_gaps(&mut self) {
        while self.ahead.remove(&(self.all_through + 1)) {
            self.all_through += 1;
        }
    }
}

/// A battle with players on other machines: every machine runs the whole simulation and they exchange only
/// commands. A guest has one link, to the hub; the hub (the host) has one per guest and passes messages
/// between them, see `Relay`.
#[derive(Resource)]
pub struct NetSession {
    relay: Relay,
    remote_teams: Vec<Team>,
    /// This machine only watches: it sends no orders and no fingerprints.
    spectating: bool,
    /// Indexed by `Team::index`.
    remote_turns: [ReceivedTurns; Team::ALL.len()],
    local_checksums: BTreeMap<u64, u64>,
    remote_checksums: BTreeMap<(u64, usize), u64>,
    pub state: SessionState,
    reported: Option<SessionState>,
    pings_in_flight: BTreeMap<u64, Instant>,
    incoming_chat: Vec<(Team, String)>,
    next_ping: u64,
    last_ping_sent: Option<Instant>,
    round_trip: Option<Duration>,
    /// On a machine that joined late, the latest tick of each team's orders the hub already had; a returning
    /// player must not send those turns again.
    turns_known_through: BTreeMap<usize, u64>,
    /// On a host in the game, where guests knock during the battle.
    door: Option<TcpListener>,
}

impl NetSession {
    /// `relays` is true for the host, which passes each guest's messages on to the other guests.
    pub fn new(links: Vec<Link>, remote_teams: Vec<Team>, relays: bool) -> Self {
        Self {
            relay: Relay::new(links, relays),
            remote_teams,
            spectating: false,
            remote_turns: Default::default(),
            local_checksums: BTreeMap::new(),
            remote_checksums: BTreeMap::new(),
            state: SessionState::InStep,
            reported: None,
            pings_in_flight: BTreeMap::new(),
            incoming_chat: Vec::new(),
            next_ping: 0,
            last_ping_sent: None,
            round_trip: None,
            turns_known_through: BTreeMap::new(),
            door: None,
        }
    }

    /// Keeps `listener` open during the battle so guests can still join, offered `level`.
    pub fn with_open_door(mut self, listener: TcpListener, level: LevelDefinition) -> Self {
        let players = std::iter::once(Team::Player)
            .chain(self.remote_teams.iter().copied())
            .collect();
        self.relay.open_door(level, players, &self.remote_teams);
        self.door = Some(listener);
        self
    }

    /// The last tick for which every other player's orders are known; a machine far behind it catches up.
    pub fn known_through(&self) -> Option<u64> {
        self.remote_teams
            .iter()
            .map(|team| self.remote_turns[team.index()].latest())
            .min()
    }

    fn admit_late_guests(&mut self) {
        if let Some(listener) = &self.door
            && self.relay.has_room()
            && let Ok((stream, _)) = listener.accept()
            && stream.set_nonblocking(false).is_ok()
            && let Ok(link) = Link::host_side(stream)
        {
            self.relay.welcome(link);
        }
        self.relay.admit_late_guests();
        self.notice_lost_players();
    }

    /// On the host: a player who lost the connection and how long the battle has waited for them.
    pub fn waiting_for_rejoin(&self) -> Option<(Team, Duration)> {
        self.relay.waiting_for_rejoin()
    }

    fn notice_lost_players(&mut self) {
        if self.relay.is_broken() && self.state == SessionState::InStep {
            self.state = SessionState::Disconnected;
        }
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
        self.relay.mark_watchers(indices);
        self
    }

    pub fn is_spectating(&self) -> bool {
        self.spectating
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
                .all(|team| self.remote_turns[team.index()].has(tick))
    }

    fn broadcast(&mut self, message: &Message) {
        self.relay.send_all(message);
        self.notice_lost_players();
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

    /// Takes in what the other machines sent about the battle.
    fn receive(&mut self, queue: &mut CommandQueue) {
        for message in self.relay.receive() {
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
                    self.remote_turns[team.index()].record(tick);
                }
                Message::CatchUp { through, turns } => {
                    for (team, last) in through {
                        self.remote_turns[team.index()].record_all_through(last);
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
                Message::Pong { nonce } => self.answered(nonce),
                Message::Ping { .. }
                | Message::Offer { .. }
                | Message::Ready
                | Message::Go
                | Message::OpenRoom { .. }
                | Message::RoomStatus { .. }
                | Message::Start
                | Message::Refused { .. } => {}
            }
        }
        self.notice_lost_players();
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
    session.relay.record_turn(local.0, turn, &commands);
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

fn pace_for_catching_up(time: &mut Time<Virtual>) {
    time.set_relative_speed(CATCH_UP_SPEED);
    time.set_max_delta(CATCH_UP_LONGEST_FRAME);
}

fn pace_normally(time: &mut Time<Virtual>, game_speed: f32) {
    time.set_relative_speed(game_speed);
    time.set_max_delta(REGULAR_LONGEST_FRAME);
}

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
            pace_for_catching_up(&mut time);
            volume.volume = bevy::audio::Volume::SILENT;
        }
        (None, true) => {
            catching_up.0 = None;
            pace_normally(&mut time, settings.game_speed_factor());
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_join_address_may_be_a_host_a_port_or_a_websocket_url_with_a_room() {
        let plain = JoinTarget::parse("192.168.1.5", 47300).unwrap();
        assert_eq!(plain.address, "192.168.1.5:47300");
        assert_eq!(plain.url, "ws://192.168.1.5:47300/");
        let with_port = JoinTarget::parse(" example.org:9000 ", 47300).unwrap();
        assert_eq!(with_port.url, "ws://example.org:9000/");
        let room = JoinTarget::parse("ws://play.example.org/room/K7Q2", 47300).unwrap();
        assert_eq!(room.address, "play.example.org:47300");
        assert_eq!(room.url, "ws://play.example.org:47300/room/K7Q2");
        let secure = JoinTarget::parse("wss://play.example.org/room/K7Q2", 47300).unwrap();
        assert_eq!(secure.address, "play.example.org:443");
        assert_eq!(secure.url, "wss://play.example.org:443/room/K7Q2");
        assert!(JoinTarget::parse("ws://", 47300).is_err());
    }
}

#[cfg(test)]
mod catching_up_tests {
    use super::*;

    fn game_time_of_one_frame(time: &mut Time<Virtual>, frame: Duration) -> Duration {
        let start = Instant::now();
        let mut real = Time::<Real>::new(start);
        real.update_with_instant(start);
        real.update_with_instant(start + frame);
        bevy::time::update_virtual_time(&mut Time::default(), time, &real);
        time.delta()
    }

    #[test]
    fn catching_up_runs_the_full_speed_even_in_a_slow_background_frame() {
        let background_frame = Duration::from_millis(33);
        let mut time = Time::<Virtual>::default();
        pace_for_catching_up(&mut time);
        let game_time = game_time_of_one_frame(&mut time, background_frame);
        assert!(game_time >= background_frame.mul_f32(CATCH_UP_SPEED * 0.99));
        pace_normally(&mut time, 1.0);
        assert_eq!(time.max_delta(), Time::<Virtual>::default().max_delta());
    }

    #[test]
    fn turns_arriving_after_a_gap_count_once_the_gap_closes() {
        let mut turns = ReceivedTurns::default();
        let first = NETWORK_COMMAND_DELAY_TICKS;
        turns.record(first + 1);
        assert!(!turns.has(first));
        assert!(turns.has(first + 1));
        assert_eq!(turns.latest(), first + 1);
        turns.record(first);
        assert_eq!(turns.all_through, first + 1);
        assert!(turns.ahead.is_empty());
    }

    #[test]
    fn a_catch_up_marks_every_turn_through_its_last_tick() {
        let mut turns = ReceivedTurns::default();
        turns.record(900);
        turns.record_all_through(500);
        assert!(turns.has(0) && turns.has(500) && !turns.has(501));
        assert_eq!(turns.latest(), 900);
    }
}
