mod common;

use bevy::prelude::*;
use common::Battle;
use iron_tide::catalog::{BuildingKind, Team, UnitKind};
use iron_tide::determinism::simulation_checksum;
use iron_tide::map::{cell_center, player_base_cell};
use iron_tide::player_commands::{GroundOrder, PlayerCommand};
use iron_tide::production::BuildItem;

const FIELD: IVec2 = IVec2::new(20, 45);

fn move_order(battle: &mut Battle, unit: Entity, goal: IVec2) -> PlayerCommand {
    PlayerCommand::Ground {
        units: vec![battle.sim_id(unit)],
        goal,
        order: GroundOrder::Move,
        queued: false,
        refinery: None,
    }
}

#[test]
fn a_command_moves_only_the_issuing_teams_units() {
    let mut battle = Battle::sandbox();
    let own = battle.spawn_unit(UnitKind::Tank, Team::Player, FIELD);
    let enemy = battle.spawn_unit(UnitKind::Tank, Team::Crimson, FIELD + IVec2::new(-12, 12));
    let goal = FIELD + IVec2::new(6, 0);
    let order = move_order(&mut battle, own, goal);
    battle.issue(Team::Player, order);
    let stolen = move_order(&mut battle, enemy, goal);
    battle.issue(Team::Player, stolen);
    let enemy_start = battle.position(enemy);
    battle.run_for(8.0);
    assert!(battle.position(own).distance(cell_center(goal)) < 2.5);
    assert!(
        battle.position(enemy).distance(enemy_start) < 0.5,
        "the player moved an enemy tank"
    );
}

#[test]
fn production_and_placement_go_through_commands() {
    let mut battle = Battle::sandbox();
    let base = player_base_cell();
    battle.construct(BuildingKind::CommandCenter, Team::Player, base);
    battle.set_credits(Team::Player, 5000);
    battle.issue(
        Team::Player,
        PlayerCommand::Enqueue {
            item: BuildItem::Structure(BuildingKind::PowerPlant),
            copies: 1,
        },
    );
    battle.run_for(BuildingKind::PowerPlant.spec().build_secs + 1.0);
    battle.issue(
        Team::Player,
        PlayerCommand::PlaceStructure {
            kind: BuildingKind::PowerPlant,
            origin: base + IVec2::new(4, 0),
        },
    );
    battle.run_for(1.0);
    assert!(
        battle
            .building(Team::Player, BuildingKind::PowerPlant)
            .is_some()
    );
}

#[test]
fn the_same_commands_on_the_same_ticks_replay_the_same_battle() {
    let play = |with_orders: bool| {
        let mut battle = Battle::standard();
        battle.run_for(5.0);
        if with_orders {
            let tanks = battle.units(Team::Player, Some(UnitKind::Tank));
            let units = tanks.iter().map(|&tank| battle.sim_id(tank)).collect();
            battle.issue(
                Team::Player,
                PlayerCommand::Ground {
                    units,
                    goal: IVec2::new(32, 32),
                    order: GroundOrder::AttackMove,
                    queued: false,
                    refinery: None,
                },
            );
        }
        battle.run_for(90.0);
        simulation_checksum(battle.world())
    };
    let first = play(true);
    assert_eq!(first, play(true));
    assert_ne!(first, play(false), "the orders changed nothing");
}
