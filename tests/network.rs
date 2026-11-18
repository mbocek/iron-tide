mod common;

use std::net::{TcpListener, TcpStream};

use bevy::prelude::*;
use common::Battle;
use iron_tide::catalog::Alliance;
use iron_tide::catalog::{Team, UnitKind};
use iron_tide::determinism::simulation_checksum;
use iron_tide::economy::Economies;
use iron_tide::levels::{BaseSite, EnemySetup, ForceSetup, LevelDefinition};
use iron_tide::lobby::NetLobby;
use iron_tide::network::{Link, NetSession, SessionState};
use iron_tide::player_commands::{GroundOrder, PlayerCommand};

const TICKS_TO_PLAY: u64 = 1200;
const LIMIT_SECS: f32 = 0.05;

/// Two battles in one process, joined over a real TCP connection: the host plays the Coalition, the guest
/// the Syndicate.
fn connected_pair() -> (Battle, Battle) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let guest_end = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let (host_end, _) = listener.accept().unwrap();
    let mut host = Battle::standard();
    let mut guest = Battle::standard();
    host.join_network(host_end, Team::Player, Team::Crimson);
    guest.join_network(guest_end, Team::Crimson, Team::Player);
    (host, guest)
}

fn attack_with_tanks(battle: &mut Battle, team: Team, goal: IVec2) {
    let tanks = battle.units(team, Some(UnitKind::Tank));
    let units = tanks.iter().map(|&tank| battle.sim_id(tank)).collect();
    battle.issue(
        team,
        PlayerCommand::Ground {
            units,
            goal,
            order: GroundOrder::AttackMove,
            queued: false,
            refinery: None,
        },
    );
}

/// Steps both battles in turns until both have played `ticks`; neither may run ahead of the other's orders.
fn play_until(host: &mut Battle, guest: &mut Battle, ticks: u64) {
    let mut rounds = 0;
    while host.tick() < ticks || guest.tick() < ticks {
        if host.tick() < ticks {
            host.run_for(LIMIT_SECS);
        }
        if guest.tick() < ticks {
            guest.run_for(LIMIT_SECS);
        }
        rounds += 1;
        assert!(rounds < 20 * ticks, "the battles stopped advancing");
        assert_eq!(host.session_state(), Some(SessionState::InStep));
        assert_eq!(guest.session_state(), Some(SessionState::InStep));
    }
}

#[test]
fn two_machines_play_the_same_battle_from_each_others_orders() {
    let (mut host, mut guest) = connected_pair();
    play_until(&mut host, &mut guest, 60);
    attack_with_tanks(&mut host, Team::Player, IVec2::new(40, 20));
    attack_with_tanks(&mut guest, Team::Crimson, IVec2::new(20, 44));
    play_until(&mut host, &mut guest, TICKS_TO_PLAY);
    while host.tick() > guest.tick() {
        guest.run_for(LIMIT_SECS);
    }
    while guest.tick() > host.tick() {
        host.run_for(LIMIT_SECS);
    }
    assert_eq!(
        simulation_checksum(host.world()),
        simulation_checksum(guest.world())
    );
    assert!(
        host.damage_taken(Team::Player) > 0.0 && host.damage_taken(Team::Crimson) > 0.0,
        "the tanks never met, so the orders did not travel"
    );
    let round_trip = host
        .world()
        .resource::<NetSession>()
        .round_trip()
        .expect("the host measured the latency");
    assert!(
        round_trip.as_millis() < 500,
        "a local round trip took {round_trip:?}"
    );
}

#[test]
fn a_battle_that_drifts_apart_is_noticed() {
    let (mut host, mut guest) = connected_pair();
    play_until(&mut host, &mut guest, 40);
    guest
        .world()
        .resource_mut::<Economies>()
        .of_mut(Team::Crimson)
        .credits += 1;
    let mut rounds = 0;
    while host.session_state() == Some(SessionState::InStep)
        || guest.session_state() == Some(SessionState::InStep)
    {
        host.run_for(LIMIT_SECS);
        guest.run_for(LIMIT_SECS);
        rounds += 1;
        assert!(rounds < 2000, "nobody noticed the drift");
    }
    assert!(matches!(
        host.session_state(),
        Some(SessionState::Desynced(_))
    ));
    assert!(matches!(
        guest.session_state(),
        Some(SessionState::Desynced(_))
    ));
}

#[test]
fn a_lost_connection_stops_the_battle() {
    let (mut host, guest) = connected_pair();
    drop(guest);
    let mut rounds = 0;
    while host.session_state() == Some(SessionState::InStep) {
        host.run_for(LIMIT_SECS);
        rounds += 1;
        assert!(rounds < 2000, "the host never noticed");
    }
    assert_eq!(host.session_state(), Some(SessionState::Disconnected));
    let stopped_at = host.tick();
    host.run_for(2.0);
    assert_eq!(
        host.tick(),
        stopped_at,
        "the battle went on without the other player"
    );
}

#[test]
fn each_player_wins_or_loses_from_their_own_side() {
    use iron_tide::components::Building;
    use iron_tide::outcome::GameOutcome;
    use iron_tide::player_commands::LocalTeam;
    let mut guest = Battle::standard();
    guest.world().insert_resource(LocalTeam(Team::Crimson));
    guest.despawn_units(Team::Player);
    let world = guest.world();
    let coalition_structures: Vec<Entity> = world
        .query::<(Entity, &Building, &Team)>()
        .iter(world)
        .filter(|(_, _, team)| **team == Team::Player)
        .map(|(entity, ..)| entity)
        .collect();
    for structure in coalition_structures {
        world.despawn(structure);
    }
    guest.run_for(2.0);
    assert_eq!(guest.outcome(), GameOutcome::Victory);
}

fn lobby_pair() -> (NetLobby, NetLobby, LevelDefinition) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap().to_string();
    let level = LevelDefinition {
        name: "Duel".into(),
        ..LevelDefinition::default()
    };
    let mut host = NetLobby::default();
    host.host_on(listener, level.clone(), 2);
    let mut guest = NetLobby::default();
    guest.join(&address);
    (host, guest, level)
}

/// Advances both lobbies on a shared clock until `done` holds.
fn settle(
    host: &mut NetLobby,
    guest: &mut NetLobby,
    clock: &mut f32,
    mut done: impl FnMut(&NetLobby, &NetLobby) -> bool,
) {
    for _ in 0..1000 {
        if done(host, guest) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
        *clock += 0.002;
        assert!(host.advance(*clock).is_none() && guest.advance(*clock).is_none());
    }
    panic!(
        "the lobbies never got there: host {:?}, guest {:?}",
        host.status(*clock),
        guest.status(*clock)
    );
}

#[test]
fn the_guest_reads_the_offer_and_both_start_after_the_countdown() {
    let (mut host, mut guest, level) = lobby_pair();
    let mut clock = 0.0;
    settle(&mut host, &mut guest, &mut clock, |_, guest| {
        guest.can_accept_offer()
    });
    assert_eq!(guest.offered_level(), Some(&level));
    guest.accept_offer();
    let counting_down = |lobby: &NetLobby| {
        lobby
            .status(0.0)
            .is_some_and(|status| status.contains("begins"))
    };
    settle(&mut host, &mut guest, &mut clock, |host, guest| {
        counting_down(host) && counting_down(guest)
    });
    let later = clock + 10.0;
    let hosted = host.advance(later).expect("the host starts");
    let joined = guest.advance(later).expect("the guest starts");
    assert_eq!(hosted.level, level);
    assert_eq!(joined.level, level);
    assert_eq!(
        (hosted.local_team, joined.local_team),
        (Team::Player, Team::Crimson)
    );
}

#[test]
fn when_the_guest_leaves_the_host_waits_for_another() {
    let (mut host, mut guest, _) = lobby_pair();
    let mut clock = 0.0;
    settle(&mut host, &mut guest, &mut clock, |_, guest| {
        guest.can_accept_offer()
    });
    guest.cancel();
    settle(&mut host, &mut guest, &mut clock, |host, _| {
        host.status(0.0)
            .is_some_and(|status| status.contains("Players 1 of 2"))
    });
    assert!(host.is_busy());
}

#[test]
fn players_can_talk_in_the_lobby() {
    let (mut host, mut guest, _) = lobby_pair();
    let mut clock = 0.0;
    settle(&mut host, &mut guest, &mut clock, |_, guest| {
        guest.can_accept_offer()
    });
    guest.draft = "hello\u{7}  there ".to_string();
    guest.say();
    settle(&mut host, &mut guest, &mut clock, |host, _| {
        !host.chat().is_empty()
    });
    let heard = &host.chat()[0];
    assert!(!heard.mine);
    assert_eq!(heard.text, "hello  there");
    assert!(guest.chat()[0].mine);
    assert!(guest.draft.is_empty());
}

fn four_connected_battles() -> Vec<(Battle, Team)> {
    let level = LevelDefinition {
        alliance: Alliance::FreeForAll,
        enemies: vec![EnemySetup::default(); 3]
            .into_iter()
            .zip([
                BaseSite::NorthEast,
                BaseSite::NorthWest,
                BaseSite::SouthEast,
            ])
            .map(|(enemy, site)| EnemySetup {
                force: ForceSetup {
                    site,
                    ..enemy.force
                },
                ..enemy
            })
            .collect(),
        ..LevelDefinition::default()
    };
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let guests = [Team::Crimson, Team::Amber, Team::Violet];
    let mut host_links = Vec::new();
    let mut battles = Vec::new();
    for &team in &guests {
        let guest_end = TcpStream::connect(address).unwrap();
        let (host_end, _) = listener.accept().unwrap();
        host_links.push(Link::new(host_end).unwrap());
        let others = Team::ALL
            .into_iter()
            .filter(|&other| other != team)
            .collect();
        let session = NetSession::new(vec![Link::new(guest_end).unwrap()], others, false);
        let mut battle = Battle::on_level(level.clone());
        battle.join_session(session, team);
        battles.push((battle, team));
    }
    let mut host = Battle::on_level(level);
    host.join_session(
        NetSession::new(host_links, guests.to_vec(), true),
        Team::Player,
    );
    battles.insert(0, (host, Team::Player));
    battles
}

#[test]
fn four_machines_play_one_free_for_all() {
    let mut battles = four_connected_battles();
    let goal = IVec2::new(32, 32);
    let mut rounds = 0;
    while battles.iter_mut().any(|(battle, _)| battle.tick() < 900) {
        for (battle, team) in &mut battles {
            if battle.tick() == 60 && !battle.units(*team, Some(UnitKind::Tank)).is_empty() {
                attack_with_tanks(battle, *team, goal);
            }
            if battle.tick() < 900 {
                battle.run_for(LIMIT_SECS);
            }
            assert_eq!(battle.session_state(), Some(SessionState::InStep));
        }
        rounds += 1;
        assert!(rounds < 20_000, "the battles stopped advancing");
    }
    let sums: Vec<u64> = battles
        .iter_mut()
        .map(|(battle, _)| {
            while battle.tick() < 900 {
                battle.run_for(LIMIT_SECS);
            }
            simulation_checksum(battle.world())
        })
        .collect();
    assert!(sums.windows(2).all(|pair| pair[0] == pair[1]), "{sums:x?}");
}

#[test]
fn three_players_take_their_seats_and_start_together() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap().to_string();
    let mut host = NetLobby::default();
    host.host_on(listener, LevelDefinition::default(), 3);
    let mut guests = [NetLobby::default(), NetLobby::default()];
    for guest in &mut guests {
        guest.join(&address);
    }
    let mut clock = 0.0;
    let mut started = Vec::new();
    for _ in 0..3000 {
        std::thread::sleep(std::time::Duration::from_millis(2));
        clock += 0.002;
        for guest in &mut guests {
            if guest.can_accept_offer() {
                guest.accept_offer();
            }
        }
        started.extend(host.advance(clock).map(|start| start.local_team));
        for guest in &mut guests {
            started.extend(guest.advance(clock).map(|start| start.local_team));
        }
        if started.len() == 3 {
            break;
        }
        let counting_down = std::iter::once(&host).chain(&guests).any(|lobby| {
            lobby
                .status(clock)
                .is_some_and(|status| status.contains("begins"))
        });
        if counting_down {
            clock += 0.5;
        }
    }
    started.sort_by_key(|team| team.index());
    assert_eq!(started, vec![Team::Player, Team::Crimson, Team::Amber]);
}
