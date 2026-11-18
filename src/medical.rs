use bevy::prelude::*;

use crate::catalog::{ArmorClass, Team};
use crate::components::{Armor, Health, Mobile};
use crate::determinism::{SimId, in_simulation_order};
use crate::map::{GameMap, world_to_cell};
use crate::movement::order_move;

/// How far a medic looks for wounded soldiers to walk over to.
const SEEK_RANGE: f32 = 12.0;
/// Close enough to treat a patient.
const TREATMENT_RANGE: f32 = 2.2;

/// A unit that restores health to wounded friendly infantry around it.
#[derive(Component)]
pub struct Healer {
    pub per_sec: f32,
    /// True while actually treating someone, which drives the kneeling animation.
    pub treating: bool,
}

pub fn treat_wounded(
    time: Res<Time>,
    map: Res<GameMap>,
    mut healers: Query<(Entity, &Team, &Transform, &mut Healer, &mut Mobile, &SimId)>,
    mut patients: Query<(Entity, &Team, &Transform, &Armor, &mut Health)>,
) {
    let dt = time.delta_secs();
    let in_order = in_simulation_order(healers.iter_mut(), |item| *item.5);
    for (healer_entity, &team, transform, mut healer, mut mobile, _) in in_order {
        let here = transform.translation;
        let patient = patients
            .iter()
            .filter(|(entity, owner, _, armor, health)| {
                *entity != healer_entity
                    && **owner == team
                    && armor.0 == ArmorClass::Infantry
                    && health.current < health.max
            })
            .map(|(entity, _, patient_transform, ..)| {
                (
                    entity,
                    patient_transform.translation.xz().distance(here.xz()),
                )
            })
            .filter(|&(_, distance)| distance <= SEEK_RANGE)
            .min_by(|a, b| a.1.total_cmp(&b.1));

        healer.treating = false;
        let Some((patient, distance)) = patient else {
            continue;
        };
        if distance <= TREATMENT_RANGE {
            if let Ok((.., mut health)) = patients.get_mut(patient) {
                health.current = (health.current + healer.per_sec * dt).min(health.max);
                healer.treating = !mobile.is_moving();
            }
        } else if !mobile.ordered_move && !mobile.is_moving() {
            let Ok((_, _, patient_transform, ..)) = patients.get(patient) else {
                continue;
            };
            order_move(
                &mut mobile,
                &map,
                here,
                world_to_cell(patient_transform.translation),
                false,
            );
        }
    }
}
