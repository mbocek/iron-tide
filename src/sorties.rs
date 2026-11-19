use bevy::prelude::*;

use crate::catalog::{BuildingKind, Team};
use crate::components::{AttackTarget, Building, Mobile, Weapon};
use crate::determinism::{SimId, in_simulation_order};
use crate::map::{GameMap, cell_center, footprint_center, world_to_cell};
use crate::movement::order_move;

/// Close enough to a Helipad's centre to be on its pad, where it is also repaired.
const ON_THE_PAD: f32 = 3.0;

/// An aircraft that strikes once and flies home to a Helipad to rearm.
#[derive(Component, Default)]
pub struct Sortie {
    /// Sitting on its pad with nothing to do, so it lands instead of hanging in the air.
    pub parked: bool,
}

/// Sends every aircraft that has dropped its load back to the nearest Helipad of its side and keeps it
/// from taking on targets until it has rearmed there; an idle one at home lands on the pad.
pub fn fly_home_to_rearm(
    map: Res<GameMap>,
    pads: Query<(&Building, &Team)>,
    mut aircraft: Query<(
        &Team,
        &Transform,
        &Weapon,
        &mut AttackTarget,
        &mut Mobile,
        &mut Sortie,
        &SimId,
    )>,
) {
    let helipads: Vec<(Team, Vec3)> = pads
        .iter()
        .filter(|(building, _)| building.kind == BuildingKind::Helipad)
        .map(|(building, &team)| {
            let footprint = building.kind.spec().footprint;
            (team, footprint_center(building.origin, footprint))
        })
        .collect();
    let in_order = in_simulation_order(aircraft.iter_mut(), |item| *item.6);
    for (&team, transform, weapon, mut attack, mut mobile, mut sortie, _) in in_order {
        let here = transform.translation;
        let home = helipads
            .iter()
            .filter(|(owner, _)| *owner == team)
            .map(|&(_, pad)| pad)
            .min_by(|a, b| {
                a.xz()
                    .distance(here.xz())
                    .total_cmp(&b.xz().distance(here.xz()))
            });
        let at_home = home.is_some_and(|pad| pad.xz().distance(here.xz()) <= ON_THE_PAD);
        sortie.parked = at_home && !mobile.is_moving() && attack.target.is_none();
        if weapon.cooldown <= 0.0 {
            continue;
        }
        attack.clear();
        let Some(pad) = home.filter(|_| !at_home) else {
            continue;
        };
        let pad_cell = world_to_cell(pad);
        if mobile.path.back() != Some(&cell_center(pad_cell)) {
            order_move(&mut mobile, &map, here, pad_cell, false);
        }
    }
}
