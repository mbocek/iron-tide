mod common;

use bevy::prelude::*;
use common::Battle;
use iron_tide::catalog::{Team, UnitKind};

const FIELD: IVec2 = IVec2::new(20, 45);

fn squad(battle: &mut Battle, kind: UnitKind, team: Team, at: IVec2, count: i32) -> Vec<Entity> {
    (0..count)
        .map(|k| battle.spawn_unit(kind, team, at + IVec2::new(0, k - count / 2)))
        .collect()
}

fn survivors(battle: &mut Battle, units: &[Entity]) -> usize {
    units.iter().filter(|&&unit| battle.alive(unit)).count()
}

#[test]
fn grenadiers_break_a_bunched_rifle_squad_of_the_same_price() {
    let mut battle = Battle::sandbox();
    let grenadiers = squad(&mut battle, UnitKind::Grenadier, Team::Crimson, FIELD, 2);
    let riflemen = squad(
        &mut battle,
        UnitKind::Rifleman,
        Team::Player,
        FIELD + IVec2::new(4, 0),
        5,
    );
    battle.run_until(40.0, |battle| {
        survivors(battle, &grenadiers) == 0 || survivors(battle, &riflemen) == 0
    });
    assert_eq!(survivors(&mut battle, &riflemen), 0, "the riflemen held");
    assert!(survivors(&mut battle, &grenadiers) > 0);
}

#[test]
fn a_tank_rolls_over_grenadiers() {
    let mut battle = Battle::sandbox();
    let grenadiers = squad(&mut battle, UnitKind::Grenadier, Team::Crimson, FIELD, 3);
    let tank = battle.spawn_unit(UnitKind::Tank, Team::Player, FIELD + IVec2::new(5, 0));
    battle.run_until(60.0, |battle| {
        survivors(battle, &grenadiers) == 0 || !battle.alive(tank)
    });
    assert!(battle.alive(tank), "the grenadiers destroyed the tank");
    assert_eq!(survivors(&mut battle, &grenadiers), 0);
}
