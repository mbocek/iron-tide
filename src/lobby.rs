use std::net::{SocketAddr, TcpListener, TcpStream, ToSocketAddrs, UdpSocket};
use std::sync::Mutex;
use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

use bevy::prelude::*;

use crate::catalog::Team;
use crate::i18n::{tr, tr_fmt};
use crate::levels::{ActiveLevel, LevelDefinition, SkirmishOptions};
use crate::match_flow::GameState;
use crate::network::{LobbyNews, NetSession};
use crate::player_commands::LocalTeam;

pub const DEFAULT_PORT: u16 = 47_300;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const COUNTDOWN_SECS: f32 = 3.0;
/// A documentation-only address: connecting a UDP socket to it sends nothing but reveals which local
/// address the computer would use, which is what the host tells the other player.
const ROUTE_PROBE: &str = "192.0.2.1:9";

#[derive(Default)]
enum LobbyPhase {
    #[default]
    Idle,
    /// The host waits for someone to connect.
    Hosting {
        listener: TcpListener,
        level: Box<LevelDefinition>,
    },
    /// The host has a guest who is looking at the offered battle.
    Negotiating {
        listener: TcpListener,
        level: Box<LevelDefinition>,
        session: Box<NetSession>,
    },
    Connecting(Mutex<Receiver<std::io::Result<TcpStream>>>),
    WaitingForOffer(Box<NetSession>),
    /// The guest decides whether to play the offered battle.
    Considering {
        level: Box<LevelDefinition>,
        session: Box<NetSession>,
    },
    Accepted {
        level: Box<LevelDefinition>,
        session: Box<NetSession>,
    },
    Countdown {
        level: Box<LevelDefinition>,
        session: Box<NetSession>,
        local_team: Team,
        starts_at: f32,
    },
    Failed(String),
}

const CHAT_LINES_SHOWN: usize = 6;

#[derive(Clone, PartialEq, Debug)]
pub struct ChatLine {
    pub mine: bool,
    pub text: String,
}

impl LobbyPhase {
    fn session_mut(&mut self) -> Option<&mut NetSession> {
        match self {
            LobbyPhase::Negotiating { session, .. }
            | LobbyPhase::WaitingForOffer(session)
            | LobbyPhase::Considering { session, .. }
            | LobbyPhase::Accepted { session, .. }
            | LobbyPhase::Countdown { session, .. } => Some(session),
            _ => None,
        }
    }
}

/// Everything needed to begin a network battle on this machine.
pub struct BattleStart {
    pub level: LevelDefinition,
    pub session: NetSession,
    pub local_team: Team,
}

/// Setting up a network battle from the main menu.
#[derive(Resource, Default)]
pub struct NetLobby {
    phase: LobbyPhase,
    /// Something the player should know that outlasts a phase, such as a guest leaving.
    notice: Option<String>,
    changed: bool,
    /// Where the other player can reach this computer, worked out once when hosting starts.
    host_address: String,
    chat: Vec<ChatLine>,
    /// The chat line being typed.
    pub draft: String,
}

impl NetLobby {
    pub fn host(&mut self, level: LevelDefinition) {
        match TcpListener::bind(("0.0.0.0", DEFAULT_PORT)) {
            Ok(listener) => self.host_on(listener, level),
            Err(error) => self.fail(tr_fmt("Cannot host: {}", &[&error])),
        }
    }

    pub fn host_on(&mut self, listener: TcpListener, level: LevelDefinition) {
        if let Err(error) = listener.set_nonblocking(true) {
            return self.fail(tr_fmt("Cannot host: {}", &[&error]));
        }
        self.notice = None;
        self.chat.clear();
        self.host_address = local_address();
        self.enter(LobbyPhase::Hosting {
            listener,
            level: Box::new(level),
        });
    }

    /// Connects in the background so the menu keeps responding; `address` may omit the port.
    pub fn join(&mut self, address: &str) {
        let address = if address.contains(':') {
            address.to_string()
        } else {
            format!("{address}:{DEFAULT_PORT}")
        };
        let (sender, receiver) = channel();
        std::thread::spawn(move || {
            let connected = address
                .to_socket_addrs()
                .and_then(|mut found| {
                    found.next().ok_or_else(|| {
                        std::io::Error::new(std::io::ErrorKind::NotFound, "no such address")
                    })
                })
                .and_then(|target: SocketAddr| {
                    TcpStream::connect_timeout(&target, CONNECT_TIMEOUT)
                });
            let _ = sender.send(connected);
        });
        self.notice = None;
        self.chat.clear();
        self.enter(LobbyPhase::Connecting(Mutex::new(receiver)));
    }

    /// The guest agrees to play the offered battle.
    pub fn accept_offer(&mut self) {
        match std::mem::take(&mut self.phase) {
            LobbyPhase::Considering { level, mut session } => {
                session.accept_offer();
                self.enter(LobbyPhase::Accepted { level, session });
            }
            other => self.phase = other,
        }
    }

    /// Stops hosting or leaves the host; the other machine learns from the closed connection.
    pub fn cancel(&mut self) {
        self.notice = None;
        self.chat.clear();
        self.enter(LobbyPhase::Idle);
    }

    pub fn can_accept_offer(&self) -> bool {
        matches!(self.phase, LobbyPhase::Considering { .. })
    }

    pub fn is_busy(&self) -> bool {
        !matches!(self.phase, LobbyPhase::Idle | LobbyPhase::Failed(_))
    }

    /// The battle on offer, for both players to read before it starts.
    pub fn offered_level(&self) -> Option<&LevelDefinition> {
        match &self.phase {
            LobbyPhase::Hosting { level, .. }
            | LobbyPhase::Negotiating { level, .. }
            | LobbyPhase::Considering { level, .. }
            | LobbyPhase::Accepted { level, .. }
            | LobbyPhase::Countdown { level, .. } => Some(level),
            _ => None,
        }
    }

    /// Whether there is another player to talk to.
    pub fn has_partner(&self) -> bool {
        matches!(
            self.phase,
            LobbyPhase::Negotiating { .. }
                | LobbyPhase::WaitingForOffer(_)
                | LobbyPhase::Considering { .. }
                | LobbyPhase::Accepted { .. }
                | LobbyPhase::Countdown { .. }
        )
    }

    /// Sends the line being typed to the other player.
    pub fn say(&mut self) {
        let draft = std::mem::take(&mut self.draft);
        if let Some(sent) = self
            .phase
            .session_mut()
            .and_then(|session| session.say(&draft))
        {
            self.remember(ChatLine {
                mine: true,
                text: sent,
            });
        }
    }

    /// The latest lines of the conversation, oldest first.
    pub fn chat(&self) -> &[ChatLine] {
        &self.chat[self.chat.len().saturating_sub(CHAT_LINES_SHOWN)..]
    }

    fn remember(&mut self, line: ChatLine) {
        self.chat.push(line);
        self.changed = true;
    }

    /// Whether the menu should be rebuilt since it last asked.
    pub fn take_change(&mut self) -> bool {
        std::mem::take(&mut self.changed)
    }

    /// What to show the player about the network battle being set up, `now` seconds into the game.
    pub fn status(&self, now: f32) -> Option<String> {
        let phase = match &self.phase {
            LobbyPhase::Idle => None,
            LobbyPhase::Hosting { .. } => Some(tr_fmt(
                "Waiting for the other player at {} (port {})",
                &[&self.host_address, &DEFAULT_PORT],
            )),
            LobbyPhase::Negotiating { .. } => {
                Some(tr("A player has joined and is looking at the battle").to_string())
            }
            LobbyPhase::Connecting(_) => Some(tr("Connecting...").to_string()),
            LobbyPhase::WaitingForOffer(_) => {
                Some(tr("Connected, waiting for the host").to_string())
            }
            LobbyPhase::Considering { .. } => {
                Some(tr("The host offers this battle. Ready to play it?").to_string())
            }
            LobbyPhase::Accepted { .. } => Some(tr("Ready, waiting for the host").to_string()),
            LobbyPhase::Countdown { starts_at, .. } => Some(tr_fmt(
                "The battle begins in {}",
                &[&((starts_at - now).max(0.0).ceil() as u32)],
            )),
            LobbyPhase::Failed(reason) => Some(reason.clone()),
        };
        match (&self.notice, phase) {
            (Some(notice), Some(phase)) => Some(format!("{notice}\n{phase}")),
            (notice, phase) => phase.or_else(|| notice.clone()),
        }
    }

    fn enter(&mut self, phase: LobbyPhase) {
        self.phase = phase;
        self.changed = true;
    }

    fn fail(&mut self, reason: String) {
        self.enter(LobbyPhase::Failed(reason));
    }

    /// Moves the lobby along at `now` seconds; returns the battle once its countdown has run out.
    pub fn advance(&mut self, now: f32) -> Option<BattleStart> {
        let phase = std::mem::take(&mut self.phase);
        let (mut next, changed) = self.step(phase, now);
        self.changed |= changed;
        let heard = next
            .session_mut()
            .map(NetSession::take_chat)
            .unwrap_or_default();
        for text in heard {
            self.remember(ChatLine { mine: false, text });
        }
        match next {
            LobbyPhase::Countdown {
                level,
                session,
                local_team,
                starts_at,
            } if now >= starts_at => {
                self.enter(LobbyPhase::Idle);
                Some(BattleStart {
                    level: *level,
                    session: *session,
                    local_team,
                })
            }
            next => {
                self.phase = next;
                None
            }
        }
    }

    fn step(&mut self, phase: LobbyPhase, now: f32) -> (LobbyPhase, bool) {
        match phase {
            LobbyPhase::Hosting { listener, level } => {
                let guest = listener.accept().ok().and_then(|(stream, _)| {
                    stream.set_nonblocking(false).ok()?;
                    NetSession::over(stream, Team::Crimson).ok()
                });
                let Some(mut session) = guest else {
                    return (LobbyPhase::Hosting { listener, level }, false);
                };
                session.offer(&level);
                self.notice = None;
                let session = Box::new(session);
                (
                    LobbyPhase::Negotiating {
                        listener,
                        level,
                        session,
                    },
                    true,
                )
            }
            LobbyPhase::Negotiating {
                listener,
                level,
                mut session,
            } => {
                let news = session.lobby_news();
                if news.contains(&LobbyNews::Left) {
                    self.notice = Some(tr("The other player left").to_string());
                    return (LobbyPhase::Hosting { listener, level }, true);
                }
                if news.contains(&LobbyNews::GuestReady) {
                    session.go();
                    return countdown(level, session, Team::Player, now);
                }
                (
                    LobbyPhase::Negotiating {
                        listener,
                        level,
                        session,
                    },
                    false,
                )
            }
            LobbyPhase::Connecting(receiver) => {
                let result = receiver
                    .lock()
                    .ok()
                    .and_then(|receiver| receiver.try_recv().ok());
                match result {
                    None => (LobbyPhase::Connecting(receiver), false),
                    Some(Ok(stream)) => match NetSession::over(stream, Team::Player) {
                        Ok(session) => (LobbyPhase::WaitingForOffer(Box::new(session)), true),
                        Err(error) => failed(tr_fmt("Connection failed: {}", &[&error])),
                    },
                    Some(Err(error)) => failed(tr_fmt("Connection failed: {}", &[&error])),
                }
            }
            LobbyPhase::WaitingForOffer(mut session) => {
                for news in session.lobby_news() {
                    match news {
                        LobbyNews::Offered(Ok(level)) => {
                            let level = Box::new(level);
                            return (LobbyPhase::Considering { level, session }, true);
                        }
                        LobbyNews::Offered(Err(version)) => {
                            return failed(tr_fmt(
                                "The host runs version {}; both players need the same version",
                                &[&version],
                            ));
                        }
                        LobbyNews::Left => return host_gone(),
                        LobbyNews::GuestReady | LobbyNews::Go => {}
                    }
                }
                (LobbyPhase::WaitingForOffer(session), false)
            }
            LobbyPhase::Considering { level, mut session } => {
                if session.lobby_news().contains(&LobbyNews::Left) {
                    return host_gone();
                }
                (LobbyPhase::Considering { level, session }, false)
            }
            LobbyPhase::Accepted { level, mut session } => {
                let news = session.lobby_news();
                if news.contains(&LobbyNews::Left) {
                    return host_gone();
                }
                if news.contains(&LobbyNews::Go) {
                    return countdown(level, session, Team::Crimson, now);
                }
                (LobbyPhase::Accepted { level, session }, false)
            }
            LobbyPhase::Countdown {
                level,
                mut session,
                local_team,
                starts_at,
            } => {
                if session.lobby_news().contains(&LobbyNews::Left) {
                    return failed(tr("The other player left").to_string());
                }
                (
                    LobbyPhase::Countdown {
                        level,
                        session,
                        local_team,
                        starts_at,
                    },
                    false,
                )
            }
            other => (other, false),
        }
    }
}

fn countdown(
    level: Box<LevelDefinition>,
    session: Box<NetSession>,
    local_team: Team,
    now: f32,
) -> (LobbyPhase, bool) {
    let starts_at = now + COUNTDOWN_SECS;
    (
        LobbyPhase::Countdown {
            level,
            session,
            local_team,
            starts_at,
        },
        true,
    )
}

fn failed(reason: String) -> (LobbyPhase, bool) {
    (LobbyPhase::Failed(reason), true)
}

fn host_gone() -> (LobbyPhase, bool) {
    failed(tr("The host closed the connection").to_string())
}

/// The battle editor's settings with a single opponent, which the other player will command.
pub fn network_level(options: &SkirmishOptions) -> LevelDefinition {
    let one_on_one = SkirmishOptions {
        enemies: 1,
        free_for_all: false,
        ..options.clone()
    };
    LevelDefinition {
        name: tr("Network battle").to_string(),
        ..one_on_one.to_level()
    }
}

fn local_address() -> String {
    UdpSocket::bind("0.0.0.0:0")
        .and_then(|socket| {
            socket.connect(ROUTE_PROBE)?;
            socket.local_addr()
        })
        .map_or_else(
            |_| tr("this computer's address").to_string(),
            |address| address.ip().to_string(),
        )
}

pub fn run_lobby(
    mut commands: Commands,
    time: Res<Time<Real>>,
    mut lobby: ResMut<NetLobby>,
    mut next_state: ResMut<NextState<GameState>>,
) {
    let Some(start) = lobby.advance(time.elapsed_secs()) else {
        return;
    };
    commands.insert_resource(ActiveLevel(start.level));
    commands.insert_resource(start.session);
    commands.insert_resource(LocalTeam(start.local_team));
    next_state.set(GameState::Playing);
}

/// Back in the main menu every battle is local again.
pub fn end_network_battle(mut commands: Commands) {
    commands.remove_resource::<NetSession>();
    commands.insert_resource(LocalTeam::default());
}
