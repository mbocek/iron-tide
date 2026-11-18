use bevy::prelude::*;

use crate::catalog::{BuildingKind, Team, UnitKind};
use crate::components::{Building, Health, Mobile, Unit};
use crate::map::footprint_center;

const SERVICE_RADIUS: f32 = 3.0;
const SERVICE_SHARE_PER_SEC: f32 = 0.04;

/// Which units a structure mends when they wait next to it (or above it).
pub fn services(structure: BuildingKind, unit: UnitKind) -> bool {
    match structure {
        BuildingKind::Helipad => unit.flies(),
        _ => false,
    }
}

pub fn service_waiting_units(
    time: Res<Time>,
    structures: Query<(&Building, &Team, &Health), Without<Unit>>,
    mut units: Query<(&Unit, &Team, &Transform, &mut Health, Option<&Mobile>)>,
) {
    let stations: Vec<(BuildingKind, Team, Vec2)> = structures
        .iter()
        .filter(|(_, _, health)| health.current > 0.0)
        .map(|(building, &team, _)| {
            let centre = footprint_center(building.origin, building.kind.spec().footprint);
            (building.kind, team, centre.xz())
        })
        .collect();
    if stations.is_empty() {
        return;
    }
    for (unit, &team, transform, mut health, mobile) in &mut units {
        let waiting = mobile.is_none_or(|mobile| !mobile.is_moving());
        if !waiting || health.current <= 0.0 || health.current >= health.max {
            continue;
        }
        let position = transform.translation.xz();
        let serviced = stations.iter().any(|&(kind, owner, centre)| {
            owner == team && services(kind, unit.0) && position.distance(centre) <= SERVICE_RADIUS
        });
        if serviced {
            let mended = health.max * SERVICE_SHARE_PER_SEC * time.delta_secs();
            health.current = (health.current + mended).min(health.max);
        }
    }
}
