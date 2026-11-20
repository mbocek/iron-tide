use std::net::{SocketAddr, TcpListener, TcpStream, ToSocketAddrs, UdpSocket};
use std::sync::Mutex;
use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

use bevy::prelude::*;

use crate::catalog::{Alliance, Team};
use crate::i18n::{tr, tr_fmt};
use crate::levels::{ActiveLevel, LevelDefinition, SkirmishOptions};
use crate::match_flow::GameState;
use crate::network::{Link, LobbyNews, MOST_SPECTATORS, NetSession, Offer};
use crate::player_commands::LocalTeam;

pub const DEFAULT_PORT: u16 = 47_300;
pub const MOST_PLAYERS: usize = 4;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const COUNTDOWN_SECS: f32 = 3.0;
const CHAT_LINES_SHOWN: usize = 6;
/// A documentation-only address: connecting a UDP socket to it sends nothing but reveals which local
/// address the computer would use, which is what the host tells the other players.
const ROUTE_PROBE: &str = "192.0.2.1:9";

/// A guest connected to the host, with the team it will play; none for a spectator.
struct Seat {
    link: Link,
    team: Option<Team>,
    ready: bool,
}

#[derive(Default)]
enum LobbyPhase {
    #[default]
    Idle,
    /// The host waits until every seat is taken and every guest is ready, then starts when it chooses,
    /// so spectators can still come in.
    Hosting {
        listener: TcpListener,
        level: Box<LevelDefinition>,
        players: Vec<Team>,
        seats: Vec<Seat>,
        start_requested: bool,
    },
    Connecting(Mutex<Receiver<std::io::Result<TcpStream>>>),
    WaitingForOffer(Link),
    /// The guest decides whether to play the offered battle.
    Considering {
        offer: Box<Offer>,
        link: Link,
    },
    Accepted {
        offer: Box<Offer>,
        link: Link,
    },
    Countdown {
        level: Box<LevelDefinition>,
        links: Vec<Link>,
        /// None on a spectator's machine.
        local_team: Option<Team>,
        remote_teams: Vec<Team>,
        relays: bool,
        /// On the host, which links lead to spectators.
        watchers: Vec<usize>,
        /// On the host, kept open so spectators can still join once the battle runs.
        door: Option<TcpListener>,
        starts_at: f32,
    },
    Failed(String),
}

#[derive(Clone, PartialEq, Debug)]
pub struct ChatLine {
    pub team: Team,
    pub mine: bool,
    pub text: String,
}

/// Everything needed to begin a network battle on this machine; a spectator's view follows the host's team.
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
    /// Where the other players can reach this computer, worked out once when hosting starts.
    host_address: String,
    chat: Vec<ChatLine>,
    /// The chat line being typed.
    pub draft: String,
}

impl NetLobby {
    /// Hosts a battle for `players` people in all, the host included.
    pub fn host(&mut self, level: LevelDefinition, players: usize) {
        match TcpListener::bind(("0.0.0.0", DEFAULT_PORT)) {
            Ok(listener) => self.host_on(listener, level, players),
            Err(error) => self.fail(tr_fmt("Cannot host: {}", &[&error])),
        }
    }

    pub fn host_on(&mut self, listener: TcpListener, level: LevelDefinition, players: usize) {
        if let Err(error) = listener.set_nonblocking(true) {
            return self.fail(tr_fmt("Cannot host: {}", &[&error]));
        }
        self.notice = None;
        self.chat.clear();
        self.host_address = local_address();
        let players = Team::ALL[..players.clamp(2, MOST_PLAYERS)].to_vec();
        self.enter(LobbyPhase::Hosting {
            listener,
            level: Box::new(level),
            players,
            seats: Vec::new(),
            start_requested: false,
        });
    }

    /// Whether every seat is taken and every guest has accepted, so the host may start.
    pub fn can_start(&self) -> bool {
        match &self.phase {
            LobbyPhase::Hosting { players, seats, .. } => everyone_ready(players, seats),
            _ => false,
        }
    }

    /// The host starts the countdown once everyone is ready.
    pub fn start_battle(&mut self) {
        let ready = self.can_start();
        if let LobbyPhase::Hosting {
            start_requested, ..
        } = &mut self.phase
        {
            *start_requested = ready;
        }
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
            LobbyPhase::Considering { offer, link } => {
                link.accept_offer();
                self.enter(LobbyPhase::Accepted { offer, link });
            }
            other => self.phase = other,
        }
    }

    /// The host changed the battle: every guest sees the new one and has to accept it again.
    pub fn reoffer(&mut self, new_level: LevelDefinition) {
        if let LobbyPhase::Hosting {
            level,
            players,
            seats,
            ..
        } = &mut self.phase
        {
            **level = new_level;
            for seat in seats.iter_mut() {
                seat.link.offer(level, seat.team, players);
                seat.ready = false;
            }
            self.changed = true;
        }
    }

    pub fn is_hosting(&self) -> bool {
        matches!(self.phase, LobbyPhase::Hosting { .. })
    }

    /// Stops hosting or leaves the host; the other machines learn from the closed connection.
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

    /// Whether there is anyone else to talk to.
    pub fn has_partner(&self) -> bool {
        match &self.phase {
            LobbyPhase::Hosting { seats, .. } => !seats.is_empty(),
            LobbyPhase::WaitingForOffer(_)
            | LobbyPhase::Considering { .. }
            | LobbyPhase::Accepted { .. }
            | LobbyPhase::Countdown { .. } => true,
            _ => false,
        }
    }

    /// The team this machine's player will command; none for a spectator.
    fn local_team(&self) -> Option<Team> {
        match &self.phase {
            LobbyPhase::Considering { offer, .. } | LobbyPhase::Accepted { offer, .. } => {
                offer.your_team
            }
            LobbyPhase::Countdown { local_team, .. } => *local_team,
            _ => Some(Team::Player),
        }
    }

    /// Sends the line being typed to everyone else.
    pub fn say(&mut self) {
        let draft = std::mem::take(&mut self.draft);
        let Some(team) = self.local_team() else {
            return;
        };
        let links: Vec<&Link> = match &self.phase {
            LobbyPhase::Hosting { seats, .. } => seats.iter().map(|seat| &seat.link).collect(),
            LobbyPhase::WaitingForOffer(link)
            | LobbyPhase::Considering { link, .. }
            | LobbyPhase::Accepted { link, .. } => vec![link],
            LobbyPhase::Countdown { links, .. } => links.iter().collect(),
            _ => Vec::new(),
        };
        let mut sent = None;
        for link in links {
            sent = link.say(team, &draft);
        }
        if let Some(text) = sent {
            self.remember(ChatLine {
                team,
                mine: true,
                text,
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

    /// The battle on offer, for every player to read before it starts.
    pub fn offered_level(&self) -> Option<&LevelDefinition> {
        match &self.phase {
            LobbyPhase::Hosting { level, .. } | LobbyPhase::Countdown { level, .. } => Some(level),
            LobbyPhase::Considering { offer, .. } | LobbyPhase::Accepted { offer, .. } => {
                Some(&offer.level)
            }
            _ => None,
        }
    }

    /// Whether the menu should be rebuilt since it last asked.
    pub fn take_change(&mut self) -> bool {
        std::mem::take(&mut self.changed)
    }

    /// What to show the player about the network battle being set up, `now` seconds into the game.
    pub fn status(&self, now: f32) -> Option<String> {
        let phase = match &self.phase {
            LobbyPhase::Idle => None,
            LobbyPhase::Hosting { players, seats, .. } => {
                let ready = seats.iter().filter(|seat| seat.ready).count();
                let seated = seats.iter().filter(|seat| seat.team.is_some()).count();
                Some(tr_fmt(
                    "Players {} of {}, {} ready. Others join at {} (port {})",
                    &[
                        &(seated + 1),
                        &players.len(),
                        &ready,
                        &self.host_address,
                        &DEFAULT_PORT,
                    ],
                ))
            }
            LobbyPhase::Connecting(_) => Some(tr("Connecting...").to_string()),
            LobbyPhase::WaitingForOffer(_) => {
                Some(tr("Connected, waiting for the host").to_string())
            }
            LobbyPhase::Considering { offer, .. } => Some(match offer.your_team {
                Some(team) => tr_fmt(
                    "The host offers this battle; you would play {}. Ready?",
                    &[&tr(team.name_in(offer.level.faction_of(team)))],
                ),
                None if offer.under_way => {
                    tr("The battle is under way; you can watch it after catching up. Ready?")
                        .to_string()
                }
                None => tr("Every seat is taken; you can watch the battle. Ready?").to_string(),
            }),
            LobbyPhase::Accepted { .. } => {
                Some(tr("Ready, waiting for the other players").to_string())
            }
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
        let next = self.step(phase, now);
        match next {
            LobbyPhase::Countdown {
                level,
                links,
                local_team,
                remote_teams,
                relays,
                watchers,
                door,
                starts_at,
            } if now >= starts_at => {
                self.enter(LobbyPhase::Idle);
                let session = match local_team {
                    Some(_) => NetSession::new(links, remote_teams, relays).with_watchers(watchers),
                    None => NetSession::watching(links, remote_teams),
                };
                let session = match door {
                    Some(listener) => session.with_open_door(listener, (*level).clone()),
                    None => session,
                };
                Some(BattleStart {
                    level: *level,
                    session,
                    local_team: local_team.unwrap_or(Team::Player),
                })
            }
            next => {
                self.phase = next;
                None
            }
        }
    }

    fn step(&mut self, phase: LobbyPhase, now: f32) -> LobbyPhase {
        match phase {
            LobbyPhase::Hosting {
                listener,
                level,
                players,
                seats,
                start_requested,
            } => self.host_step(listener, level, players, seats, start_requested, now),
            LobbyPhase::Connecting(receiver) => {
                let result = receiver
                    .lock()
                    .ok()
                    .and_then(|receiver| receiver.try_recv().ok());
                match result {
                    None => LobbyPhase::Connecting(receiver),
                    Some(Ok(stream)) => match Link::new(stream) {
                        Ok(link) => self.changed_to(LobbyPhase::WaitingForOffer(link)),
                        Err(error) => self.failed(tr_fmt("Connection failed: {}", &[&error])),
                    },
                    Some(Err(error)) => self.failed(tr_fmt("Connection failed: {}", &[&error])),
                }
            }
            LobbyPhase::WaitingForOffer(mut link) => {
                for news in link.lobby_news() {
                    match news {
                        LobbyNews::Offered(Ok(offer)) => {
                            return self.changed_to(LobbyPhase::Considering { offer, link });
                        }
                        LobbyNews::Offered(Err(version)) => {
                            return self.failed(tr_fmt(
                                "The host runs version {}; all players need the same version",
                                &[&version],
                            ));
                        }
                        LobbyNews::Left => return self.host_gone(),
                        LobbyNews::Chat(team, text) => self.heard(team, text),
                        LobbyNews::GuestReady | LobbyNews::Go => {}
                    }
                }
                LobbyPhase::WaitingForOffer(link)
            }
            LobbyPhase::Considering {
                mut offer,
                mut link,
            } => {
                for news in link.lobby_news() {
                    match news {
                        LobbyNews::Left => return self.host_gone(),
                        LobbyNews::Chat(team, text) => self.heard(team, text),
                        LobbyNews::Offered(Ok(changed)) => offer = self.battle_changed(changed),
                        _ => {}
                    }
                }
                LobbyPhase::Considering { offer, link }
            }
            LobbyPhase::Accepted { offer, mut link } => {
                let mut go = false;
                for news in link.lobby_news() {
                    match news {
                        LobbyNews::Left => return self.host_gone(),
                        LobbyNews::Chat(team, text) => self.heard(team, text),
                        LobbyNews::Offered(Ok(changed)) => {
                            let offer = self.battle_changed(changed);
                            return LobbyPhase::Considering { offer, link };
                        }
                        LobbyNews::Go => go = true,
                        _ => {}
                    }
                }
                if !go {
                    return LobbyPhase::Accepted { offer, link };
                }
                let Offer {
                    level,
                    your_team,
                    players,
                    under_way,
                } = *offer;
                let remote_teams = players
                    .into_iter()
                    .filter(|&team| Some(team) != your_team)
                    .collect();
                let countdown = if under_way { 0.0 } else { COUNTDOWN_SECS };
                self.changed_to(LobbyPhase::Countdown {
                    level: Box::new(level),
                    links: vec![link],
                    local_team: your_team,
                    remote_teams,
                    relays: false,
                    watchers: Vec::new(),
                    door: None,
                    starts_at: now + countdown,
                })
            }
            LobbyPhase::Countdown {
                level,
                mut links,
                local_team,
                remote_teams,
                relays,
                watchers,
                door,
                starts_at,
            } => {
                let left = links
                    .iter_mut()
                    .flat_map(Link::lobby_news)
                    .any(|news| news == LobbyNews::Left);
                if left {
                    return self.failed(tr("A player left before the battle began").to_string());
                }
                LobbyPhase::Countdown {
                    level,
                    links,
                    local_team,
                    remote_teams,
                    relays,
                    watchers,
                    door,
                    starts_at,
                }
            }
            other => other,
        }
    }

    fn host_step(
        &mut self,
        listener: TcpListener,
        level: Box<LevelDefinition>,
        players: Vec<Team>,
        mut seats: Vec<Seat>,
        start_requested: bool,
        now: f32,
    ) -> LobbyPhase {
        let free_team = players
            .iter()
            .skip(1)
            .copied()
            .find(|team| seats.iter().all(|seat| seat.team != Some(*team)));
        let spectators = seats.iter().filter(|seat| seat.team.is_none()).count();
        let room = free_team.is_some() || spectators < MOST_SPECTATORS;
        if room && let Some(link) = accept_guest(&listener) {
            let team = free_team;
            link.offer(&level, team, &players);
            seats.push(Seat {
                link,
                team,
                ready: false,
            });
            self.notice = None;
            self.changed = true;
        }
        let mut index = 0;
        while index < seats.len() {
            let team = seats[index].team;
            let mut left = false;
            for news in seats[index].link.lobby_news() {
                match news {
                    LobbyNews::GuestReady => {
                        seats[index].ready = true;
                        self.changed = true;
                    }
                    LobbyNews::Chat(speaker, text) => {
                        for (other, seat) in seats.iter().enumerate() {
                            if other != index {
                                seat.link.relay_chat(speaker, &text);
                            }
                        }
                        self.heard(speaker, text);
                    }
                    LobbyNews::Left => left = true,
                    LobbyNews::Offered(_) | LobbyNews::Go => {}
                }
            }
            if left {
                seats.remove(index);
                self.notice = Some(match team {
                    Some(team) => tr_fmt("{} left", &[&tr(team.name_in(level.faction_of(team)))]),
                    None => tr("A spectator left").to_string(),
                });
                self.changed = true;
            } else {
                index += 1;
            }
        }
        if !(start_requested && everyone_ready(&players, &seats)) {
            return LobbyPhase::Hosting {
                listener,
                level,
                players,
                seats,
                start_requested: false,
            };
        }
        for seat in &seats {
            seat.link.go();
        }
        let remote_teams = seats.iter().filter_map(|seat| seat.team).collect();
        let watchers = seats
            .iter()
            .enumerate()
            .filter(|(_, seat)| seat.team.is_none())
            .map(|(index, _)| index)
            .collect();
        self.changed_to(LobbyPhase::Countdown {
            level,
            links: seats.into_iter().map(|seat| seat.link).collect(),
            local_team: Some(Team::Player),
            remote_teams,
            relays: true,
            watchers,
            door: Some(listener),
            starts_at: now + COUNTDOWN_SECS,
        })
    }

    fn battle_changed(&mut self, offer: Box<Offer>) -> Box<Offer> {
        self.notice = Some(tr("The host changed the battle").to_string());
        self.changed = true;
        offer
    }

    fn heard(&mut self, team: Team, text: String) {
        self.remember(ChatLine {
            team,
            mine: false,
            text,
        });
    }

    fn changed_to(&mut self, phase: LobbyPhase) -> LobbyPhase {
        self.changed = true;
        phase
    }

    fn failed(&mut self, reason: String) -> LobbyPhase {
        self.changed_to(LobbyPhase::Failed(reason))
    }

    fn host_gone(&mut self) -> LobbyPhase {
        self.failed(tr("The host closed the connection").to_string())
    }
}

fn everyone_ready(players: &[Team], seats: &[Seat]) -> bool {
    let seated = seats.iter().filter(|seat| seat.team.is_some()).count();
    seated + 1 == players.len() && seats.iter().all(|seat| seat.ready)
}

fn accept_guest(listener: &TcpListener) -> Option<Link> {
    let (stream, _) = listener.accept().ok()?;
    stream.set_nonblocking(false).ok()?;
    Link::new(stream).ok()
}

/// The battle editor's settings for a network battle: every person gets a corner, the computer takes any
/// corners left over.
pub fn network_level(options: &SkirmishOptions, players: usize) -> LevelDefinition {
    let opponents = options.enemies.max(players.saturating_sub(1));
    let alliance = if players > 2 && options.alliance == Alliance::CoalitionAgainstSyndicate {
        Alliance::FreeForAll
    } else {
        options.alliance
    };
    let options = SkirmishOptions {
        enemies: opponents,
        alliance,
        ..options.clone()
    };
    LevelDefinition {
        name: tr("Network battle").to_string(),
        ..options.to_level()
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
