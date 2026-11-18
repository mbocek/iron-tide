mod common;

use bevy::prelude::*;
use common::Battle;
use iron_tide::catalog::{Team, UnitKind};
use iron_tide::components::{AttackTarget, Health, Mobile};
use iron_tide::map::{GameMap, cell_center};

const NORTH_BANK: IVec2 = IVec2::new(20, 24);
const SOUTH_BANK: IVec2 = IVec2::new(20, 40);
const FIELD: IVec2 = IVec2::new(20, 45);

fn health(battle: &mut Battle, entity: Entity) -> f32 {
    battle
        .world()
        .get::<Health>(entity)
        .map_or(0.0, |h| h.current)
}

#[test]
fn a_gunship_flies_straight_over_the_river_at_altitude() {
    let mut battle = Battle::sandbox();
    let crossing_blocked = (NORTH_BANK.y..=SOUTH_BANK.y).any(|row| {
        !battle
            .world()
            .resource::<GameMap>()
            .is_passable(IVec2::new(20, row))
    });
    assert!(
        crossing_blocked,
        "the test needs the river between the banks"
    );

    let gunship = battle.spawn_unit(UnitKind::Gunship, Team::Player, NORTH_BANK);
    battle.order_move(gunship, SOUTH_BANK);
    assert_eq!(battle.world().get::<Mobile>(gunship).unwrap().path.len(), 1);
    let flight_secs =
        cell_center(NORTH_BANK).distance(cell_center(SOUTH_BANK)) / UnitKind::Gunship.spec().speed;
    battle.run_for(flight_secs + 3.0);

    let position = battle.position(gunship);
    assert!(
        position.xz().distance(cell_center(SOUTH_BANK).xz()) < 1.5,
        "landed at {position}"
    );
    assert!(position.y > 5.0, "flying at only {} m", position.y);
}

#[test]
fn tanks_cannot_hit_a_gunship_but_rocketeers_can() {
    let mut battle = Battle::sandbox();
    let tank = battle.spawn_unit(UnitKind::Tank, Team::Player, FIELD);
    let gunship = battle.spawn_unit(UnitKind::Gunship, Team::Crimson, FIELD + IVec2::new(4, 0));
    battle.run_for(6.0);
    let tank_target = battle
        .world()
        .get::<AttackTarget>(tank)
        .and_then(|a| a.target);
    assert!(tank_target.is_none(), "the tank aimed at an aircraft");
    let full = UnitKind::Gunship.spec().max_health;
    assert_eq!(health(&mut battle, gunship), full);
    assert!(
        health(&mut battle, tank) < UnitKind::Tank.spec().max_health,
        "the gunship did not attack"
    );

    battle.spawn_unit(UnitKind::Rocketeer, Team::Player, FIELD + IVec2::new(1, 2));
    battle.run_for(6.0);
    assert!(
        health(&mut battle, gunship) < full,
        "rocketeers could not hit the gunship"
    );
}
