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
