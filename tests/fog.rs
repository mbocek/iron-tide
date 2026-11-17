mod common;

use bevy::prelude::*;
use common::Battle;
use iron_tide::catalog::{BuildingKind, Team, UnitKind};
use iron_tide::combat::Targets;
use iron_tide::components::{Building, Health};
use iron_tide::fog::{CellSight, FogOfWar};
use iron_tide::levels::LevelDefinition;
use iron_tide::map::BASE_SITES;
use iron_tide::savegame::SaveRequests;

const OPEN_FIELD: IVec2 = IVec2::new(30, 45);

fn seen_by_player(battle: &mut Battle, entity: Entity) -> bool {
    battle
        .world()
        .resource::<Targets>()
        .get(entity)
        .is_some_and(|target| target.seen_by_player)
}

fn sight_at(battle: &mut Battle, cell: IVec2) -> CellSight {
    battle.world().resource::<FogOfWar>().sight_at(cell)
}

#[test]
fn the_enemy_base_starts_hidden_and_the_own_base_visible() {
    let mut battle = Battle::standard();
    battle.run_for(0.5);
    assert_eq!(sight_at(&mut battle, BASE_SITES[0]), CellSight::Visible);
    assert_eq!(sight_at(&mut battle, BASE_SITES[1]), CellSight::Unexplored);
    for enemy in battle.units(Team::Crimson, None) {
        assert!(
            !seen_by_player(&mut battle, enemy),
            "enemy units are visible through the fog"
        );
    }
}

#[test]
fn units_reveal_the_ground_they_cross_and_remember_it() {
    let mut battle = Battle::sandbox();
    let scout = battle.spawn_unit(UnitKind::Scout, Team::Player, IVec2::new(20, 50));
    let far_point = IVec2::new(45, 50);
    battle.run_for(0.5);
    assert_eq!(sight_at(&mut battle, far_point), CellSight::Unexplored);

    battle.order_move(scout, far_point);
    battle.run_for(10.0);
    assert_eq!(sight_at(&mut battle, far_point), CellSight::Visible);

    battle.order_move(scout, IVec2::new(20, 50));
    battle.run_for(10.0);
    assert_eq!(
        sight_at(&mut battle, far_point),
        CellSight::Explored,
        "explored ground is remembered"
    );
}

#[test]
fn artillery_needs_a_spotter_to_fire_beyond_its_own_sight() {
    let mut battle = Battle::sandbox();
    battle.spawn_unit(UnitKind::Artillery, Team::Player, OPEN_FIELD);
    battle.construct(
        BuildingKind::PowerPlant,
        Team::Crimson,
        OPEN_FIELD + IVec2::new(8, -1),
    );
    let world = battle.world();
    let target = world
        .query_filtered::<Entity, With<Building>>()
        .single(world)
        .unwrap();
    let full_health = BuildingKind::PowerPlant.spec().max_health;

    battle.run_for(10.0);
    assert!(!seen_by_player(&mut battle, target));
    assert_eq!(
        battle.world().get::<Health>(target).unwrap().current,
        full_health,
        "fired at an unseen target"
    );

    battle.spawn_unit(
        UnitKind::Rifleman,
        Team::Player,
        OPEN_FIELD + IVec2::new(4, 0),
    );
    battle.run_for(10.0);
    let health = battle
        .world()
        .get::<Health>(target)
        .map_or(0.0, |h| h.current);
    assert!(
        health < full_health,
        "the spotter's view did not let the artillery fire"
    );
}

#[test]
fn levels_can_switch_the_fog_off() {
    let mut battle = Battle::on_level(LevelDefinition {
        fog_of_war: false,
        ..Default::default()
    });
    battle.run_for(0.5);
    assert_eq!(sight_at(&mut battle, BASE_SITES[1]), CellSight::Visible);
    for enemy in battle.units(Team::Crimson, None) {
        assert!(seen_by_player(&mut battle, enemy));
    }
}

#[test]
fn exploration_is_kept_in_save_games() {
    let mut battle = Battle::sandbox();
    let scout = battle.spawn_unit(UnitKind::Scout, Team::Player, IVec2::new(20, 50));
    let far_point = IVec2::new(45, 50);
    battle.order_move(scout, far_point);
    battle.run_for(10.0);
    battle.world().resource_mut::<SaveRequests>().save = Some(3);
    battle.run_for(0.1);

    battle.world().resource_mut::<SaveRequests>().load = Some(3);
    battle.run_for(0.5);
    assert_ne!(sight_at(&mut battle, far_point), CellSight::Unexplored);
}
