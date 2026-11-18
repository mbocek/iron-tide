use std::net::{SocketAddr, TcpListener, TcpStream, ToSocketAddrs, UdpSocket};
use std::sync::Mutex;
use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

use bevy::prelude::*;

use crate::catalog::Team;
use crate::i18n::{tr, tr_fmt};
use crate::levels::{ActiveLevel, LevelDefinition, SkirmishOptions};
use crate::match_flow::GameState;
use crate::network::{NetSession, SessionState};
use crate::player_commands::LocalTeam;

pub const DEFAULT_PORT: u16 = 47_300;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// A documentation-only address: connecting a UDP socket to it sends nothing but reveals which local
/// address the computer would use, which is what the host tells the other player.
const ROUTE_PROBE: &str = "192.0.2.1:9";

#[derive(Default)]
pub enum LobbyPhase {
    #[default]
    Idle,
    Hosting {
        listener: TcpListener,
        level: Box<LevelDefinition>,
    },
    Connecting(Mutex<Receiver<std::io::Result<TcpStream>>>),
    WaitingForStart(Box<NetSession>),
    Failed(String),
}

/// Setting up a network battle from the main menu.
#[derive(Resource, Default)]
pub struct NetLobby {
    pub phase: LobbyPhase,
}

impl NetLobby {
    pub fn host(&mut self, level: LevelDefinition) {
        self.phase = match TcpListener::bind(("0.0.0.0", DEFAULT_PORT)).and_then(|listener| {
            listener.set_nonblocking(true)?;
            Ok(listener)
        }) {
            Ok(listener) => LobbyPhase::Hosting {
                listener,
                level: Box::new(level),
            },
            Err(error) => LobbyPhase::Failed(tr_fmt("Cannot host: {}", &[&error])),
        };
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
        self.phase = LobbyPhase::Connecting(Mutex::new(receiver));
    }

    pub fn cancel(&mut self) {
        self.phase = LobbyPhase::Idle;
    }

    /// What to show the player about the network battle being set up.
    pub fn status(&self) -> Option<String> {
        match &self.phase {
            LobbyPhase::Idle => None,
            LobbyPhase::Hosting { .. } => Some(tr_fmt(
                "Waiting for the other player at {} (port {})",
                &[&local_address(), &DEFAULT_PORT],
            )),
            LobbyPhase::Connecting(_) => Some(tr("Connecting...").to_string()),
            LobbyPhase::WaitingForStart(_) => {
                Some(tr("Connected, waiting for the host").to_string())
            }
            LobbyPhase::Failed(reason) => Some(reason.clone()),
        }
    }
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

/// Moves the lobby along: the host accepts the guest and starts, the guest waits for the host's battle.
pub fn run_lobby(
    mut commands: Commands,
    mut lobby: ResMut<NetLobby>,
    mut next_state: ResMut<NextState<GameState>>,
) {
    let phase = std::mem::take(&mut lobby.phase);
    lobby.phase = match phase {
        LobbyPhase::Hosting { listener, level } => match listener.accept() {
            Ok((stream, _)) => match stream
                .set_nonblocking(false)
                .and_then(|_| NetSession::over(stream, Team::Crimson))
            {
                Ok(mut session) => {
                    session.start(&level);
                    begin_battle(
                        &mut commands,
                        &mut next_state,
                        *level,
                        session,
                        Team::Player,
                    );
                    LobbyPhase::Idle
                }
                Err(error) => LobbyPhase::Failed(tr_fmt("Connection failed: {}", &[&error])),
            },
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                LobbyPhase::Hosting { listener, level }
            }
            Err(error) => LobbyPhase::Failed(tr_fmt("Connection failed: {}", &[&error])),
        },
        LobbyPhase::Connecting(receiver) => {
            let result = receiver
                .lock()
                .ok()
                .and_then(|receiver| receiver.try_recv().ok());
            match result {
                None => LobbyPhase::Connecting(receiver),
                Some(Ok(stream)) => match NetSession::over(stream, Team::Player) {
                    Ok(session) => LobbyPhase::WaitingForStart(Box::new(session)),
                    Err(error) => LobbyPhase::Failed(tr_fmt("Connection failed: {}", &[&error])),
                },
                Some(Err(error)) => LobbyPhase::Failed(tr_fmt("Connection failed: {}", &[&error])),
            }
        }
        LobbyPhase::WaitingForStart(mut session) => match session.started_level() {
            Some(Ok(level)) => {
                begin_battle(
                    &mut commands,
                    &mut next_state,
                    level,
                    *session,
                    Team::Crimson,
                );
                LobbyPhase::Idle
            }
            Some(Err(version)) => LobbyPhase::Failed(tr_fmt(
                "The host runs version {}; both players need the same version",
                &[&version],
            )),
            None if session.state == SessionState::Disconnected => {
                LobbyPhase::Failed(tr("The host closed the connection").to_string())
            }
            None => LobbyPhase::WaitingForStart(session),
        },
        other => other,
    };
}

fn begin_battle(
    commands: &mut Commands,
    next_state: &mut NextState<GameState>,
    level: LevelDefinition,
    session: NetSession,
    local_team: Team,
) {
    commands.insert_resource(ActiveLevel(level));
    commands.insert_resource(session);
    commands.insert_resource(LocalTeam(local_team));
    next_state.set(GameState::Playing);
}

/// Back in the main menu every battle is local again.
pub fn end_network_battle(mut commands: Commands) {
    commands.remove_resource::<NetSession>();
    commands.insert_resource(LocalTeam::default());
}
