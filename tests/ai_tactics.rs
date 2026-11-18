mod common;

use bevy::prelude::*;
use common::{Battle, BattleSetup};
use iron_tide::catalog::{BuildingKind, Team, UnitKind};
use iron_tide::components::{Building, Health, Unit};
use iron_tide::engineers::EngineerTask;
use iron_tide::levels::{Difficulty, LevelDefinition};
use iron_tide::map::{footprint_center, world_to_cell};

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

fn crimson_buildings(battle: &mut Battle, kind: BuildingKind) -> usize {
    battle
        .buildings(Team::Crimson)
        .into_iter()
        .filter(|&built| built == kind)
        .count()
}

#[test]
fn the_computer_answers_enemy_gunships_with_flak() {
    let mut battle = fogless_match();
    let command_center = battle
        .building(Team::Player, BuildingKind::CommandCenter)
        .unwrap();
    let home = world_to_cell(battle.position(command_center));
    for offset in 0..4 {
        battle.spawn_unit(
            UnitKind::Gunship,
            Team::Player,
            home + IVec2::new(-3, offset),
        );
    }
    let took = battle.run_until(6.0 * 60.0, |battle| {
        crimson_buildings(battle, BuildingKind::FlakCannon) >= 2
    });
    assert!(
        took.is_some(),
        "the computer built {} flak cannons against four gunships",
        crimson_buildings(&mut battle, BuildingKind::FlakCannon)
    );
}

#[test]
fn the_computer_sends_badly_damaged_vehicles_to_its_repair_bay() {
    let mut battle = Battle::sandbox();
    let base = IVec2::new(40, 12);
    battle.construct(BuildingKind::CommandCenter, Team::Crimson, base);
    let bay_origin = base + IVec2::new(-5, 0);
    battle.construct(BuildingKind::RepairBay, Team::Crimson, bay_origin);
    battle.add_ai(Team::Crimson, base, Difficulty::Normal);
    let tank = battle.spawn_unit(UnitKind::Tank, Team::Crimson, base + IVec2::new(-6, 12));
    {
        let mut health = battle.world().get_mut::<Health>(tank).unwrap();
        health.current = health.max * 0.2;
    }
    let bay_centre = footprint_center(bay_origin, BuildingKind::RepairBay.spec().footprint);
    let took = battle.run_until(60.0, |battle| {
        let health = battle.world().get::<Health>(tank).unwrap();
        health.current / health.max > 0.85
    });
    assert!(took.is_some(), "the tank was never repaired");
    assert!(battle.position(tank).xz().distance(bay_centre.xz()) < 5.0);
}
