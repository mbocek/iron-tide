mod common;

use common::Battle;
use iron_tide::catalog::Team;
use iron_tide::determinism::simulation_checksum;
use iron_tide::levels::{BaseSite, Difficulty};

const CHECK_EVERY_SECS: f32 = 15.0;
const MATCH_SECS: f32 = 8.0 * 60.0;

fn computer_against_computer() -> Battle {
    let mut battle = Battle::standard();
    battle.add_ai(Team::Player, BaseSite::SouthWest.origin(), Difficulty::Hard);
    battle
}

#[test]
fn the_same_battle_plays_out_identically_twice() {
    let mut first = computer_against_computer();
    let mut second = computer_against_computer();
    let mut elapsed = 0.0;
    while elapsed < MATCH_SECS {
        first.run_for(CHECK_EVERY_SECS);
        second.run_for(CHECK_EVERY_SECS);
        elapsed += CHECK_EVERY_SECS;
        assert_eq!(
            simulation_checksum(first.world()),
            simulation_checksum(second.world()),
            "the battles drifted apart by {elapsed} s"
        );
    }
    assert!(
        first.damage_taken(Team::Player) + first.damage_taken(Team::Crimson) > 0.0,
        "nothing happened, so the comparison proves little"
    );
}

/// Stands in for presentation, which adds its own components at moments that differ between machines.
#[derive(bevy::prelude::Component)]
struct DrawnBySomeone;

fn shuffle_query_order(battle: &mut Battle, round: u32) {
    use bevy::prelude::*;
    use iron_tide::combat::Projectile;
    use iron_tide::components::{Building, Unit};
    let world = battle.world();
    let units: Vec<Entity> = world
        .query_filtered::<Entity, Or<(With<Unit>, With<Building>, With<Projectile>)>>()
        .iter(world)
        .collect();
    for (index, unit) in units.into_iter().enumerate() {
        if (index as u32 + round).is_multiple_of(3) {
            if world.entity(unit).contains::<DrawnBySomeone>() {
                world.entity_mut(unit).remove::<DrawnBySomeone>();
            } else {
                world.entity_mut(unit).insert(DrawnBySomeone);
            }
        }
    }
}

#[test]
fn presentation_reordering_units_does_not_change_the_battle() {
    let mut plain = computer_against_computer();
    let mut drawn = computer_against_computer();
    let mut elapsed = 0.0;
    let mut round = 0;
    while elapsed < MATCH_SECS {
        for _ in 0..(CHECK_EVERY_SECS as u32) {
            plain.run_for(1.0);
            shuffle_query_order(&mut drawn, round);
            drawn.run_for(1.0);
            round += 1;
        }
        elapsed += CHECK_EVERY_SECS;
        assert_eq!(
            simulation_checksum(plain.world()),
            simulation_checksum(drawn.world()),
            "unit order changed the battle by {elapsed} s"
        );
    }
}
