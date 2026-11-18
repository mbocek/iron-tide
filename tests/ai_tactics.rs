mod common;

use bevy::prelude::*;
use common::{Battle, BattleSetup};
use iron_tide::catalog::{BuildingKind, Team, UnitKind};
use iron_tide::components::{Building, Unit};
use iron_tide::engineers::EngineerTask;
use iron_tide::levels::LevelDefinition;

fn fogless_match() -> Battle {
    Battle::new(BattleSetup {
        enemy_ai: true,
        starting_forces: true,
        level: Some(LevelDefinition {
            fog_of_war: false,
            ..LevelDefinition::default()
        }),
    })
}

fn crimson_walls_beside_turrets(battle: &mut Battle) -> usize {
    let world = battle.world();
    let crimson: Vec<(BuildingKind, IVec2)> = world
        .query::<(&Building, &Team)>()
        .iter(world)
        .filter(|(_, team)| **team == Team::Crimson)
        .map(|(building, _)| (building.kind, building.origin))
        .collect();
    let turrets: Vec<IVec2> = crimson
        .iter()
        .filter(|(kind, _)| *kind == BuildingKind::GunTurret)
        .map(|&(_, origin)| origin)
        .collect();
    crimson
        .iter()
        .filter(|(kind, origin)| {
            *kind == BuildingKind::Wall
                && turrets
                    .iter()
                    .any(|turret| (*turret - *origin).abs().max_element() == 1)
        })
        .count()
}

#[test]
fn the_computer_shields_its_turrets_with_walls() {
    let mut battle = fogless_match();
    let took = battle.run_until(12.0 * 60.0, |battle| {
        crimson_walls_beside_turrets(battle) >= 2
    });
    assert!(
        took.is_some(),
        "no walls appeared beside the computer's turrets"
    );
}

#[test]
fn the_computer_sends_an_engineer_after_a_known_structure() {
    let mut battle = fogless_match();
    let took = battle.run_until(15.0 * 60.0, |battle| {
        let world = battle.world();
        world
            .query::<(&Unit, &Team, Has<EngineerTask>)>()
            .iter(world)
            .any(|(unit, team, tasked)| {
                *team == Team::Crimson && unit.0 == UnitKind::Engineer && tasked
            })
    });
    assert!(took.is_some(), "the computer never sent an engineer");
}
