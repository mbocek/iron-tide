mod common;

use bevy::prelude::*;
use common::Battle;
use iron_tide::catalog::{BuildingKind, Team, UnitKind};
use iron_tide::match_stats::MatchStats;
use iron_tide::production::BuildItem;

const FIELD: IVec2 = IVec2::new(20, 45);

#[test]
fn the_tally_counts_kills_losses_damage_and_new_units() {
    let mut battle = Battle::sandbox();
    battle.run_for(1.0);
    battle.spawn_unit(UnitKind::Tank, Team::Player, FIELD);
    let prey = battle.spawn_unit(UnitKind::Harvester, Team::Crimson, FIELD + IVec2::new(4, 0));
    battle.run_until(120.0, |battle| !battle.alive(prey));
    battle.run_for(0.5);

    let stats = battle.world().resource::<MatchStats>();
    let player = *stats.of(Team::Player);
    let crimson = *stats.of(Team::Crimson);
    assert_eq!(player.enemies_destroyed, 1);
    assert_eq!(crimson.units_lost, 1);
    assert!(player.damage_dealt >= UnitKind::Harvester.spec().max_health);
    assert_eq!(crimson.damage_taken, player.damage_dealt);
    assert_eq!(
        player.units_trained, 1,
        "the tank spawned after the start counts as trained"
    );

    battle.construct(BuildingKind::PowerPlant, Team::Player, IVec2::new(6, 47));
    battle.construct(BuildingKind::Barracks, Team::Player, IVec2::new(10, 52));
    battle.set_credits(Team::Player, 1000);
    assert!(battle.enqueue(Team::Player, BuildItem::Unit(UnitKind::Rifleman)));
    battle.run_for(UnitKind::Rifleman.spec().build_secs + 1.0);
    let player = *battle.world().resource::<MatchStats>().of(Team::Player);
    assert_eq!(player.units_trained, 2);
    assert_eq!(player.structures_built, 2);
}
