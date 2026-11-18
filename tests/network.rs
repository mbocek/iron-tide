mod common;

use std::net::{TcpListener, TcpStream};

use bevy::prelude::*;
use common::Battle;
use iron_tide::catalog::{Team, UnitKind};
use iron_tide::determinism::simulation_checksum;
use iron_tide::economy::Economies;
use iron_tide::network::SessionState;
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
