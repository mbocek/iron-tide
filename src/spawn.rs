use bevy::prelude::*;

use crate::animation::{animated_building_model, animated_unit_model};
use crate::assets::ModelLibrary;
use crate::catalog::{ArmorClass, BuildingKind, Team, UnitKind};
use crate::components::*;
use crate::fog::Sight;
use crate::map::{CELL_SIZE, GameMap, footprint_center};
use crate::medical::Healer;

const TURRET_TURN_RATE: f32 = 3.5;

pub fn spawn_unit(
    commands: &mut Commands,
    models: &ModelLibrary,
    kind: UnitKind,
    team: Team,
    position: Vec3,
    facing: f32,
) -> Entity {
    let spec = kind.spec();
    let mut unit = commands.spawn((
        Unit(kind),
        team,
        Health::full(spec.max_health),
        BodyRadius(spec.radius),
        Sight(spec.sight),
        Armor(spec.armor),
        Mobile::new(spec.speed, spec.turn_rate, facing),
        Orders::default(),
        WorldAssetRoot(models.unit(kind, team)),
        animated_unit_model(kind, team),
        Transform::from_translation(position).with_rotation(Quat::from_rotation_y(facing)),
    ));
    if let Some(weapon) = spec.weapon {
        unit.insert((
            Weapon {
                spec: weapon,
                cooldown: 0.0,
            },
            AttackTarget::default(),
        ));
    }
    if spec.has_turret {
        unit.insert(Turret::new(facing, TURRET_TURN_RATE));
    }
    if spec.heal_per_sec > 0.0 {
        unit.insert(Healer {
            per_sec: spec.heal_per_sec,
            treating: false,
        });
    }
    if kind == UnitKind::Harvester {
        unit.insert(Harvester {
            state: HarvestState::SeekingField,
            cargo: 0,
            preferred_field: None,
        });
    }
    unit.id()
}

pub fn spawn_building(
    commands: &mut Commands,
    models: &ModelLibrary,
    map: &mut GameMap,
    kind: BuildingKind,
    team: Team,
    origin: IVec2,
) -> Entity {
    let spec = kind.spec();
    let footprint_radius = spec.footprint.as_vec2().max_element() * CELL_SIZE * 0.5;
    let mut building = commands.spawn((
        Building { kind, origin },
        team,
        Health::full(spec.max_health),
        BodyRadius(footprint_radius),
        Sight(spec.sight),
        Armor(ArmorClass::Structure),
        WorldAssetRoot(models.building(kind, team)),
        Transform::from_translation(footprint_center(origin, spec.footprint)),
    ));
    if let Some(weapon) = spec.weapon {
        building.insert((
            Weapon {
                spec: weapon,
                cooldown: 0.0,
            },
            AttackTarget::default(),
            Turret::new(0.0, TURRET_TURN_RATE),
        ));
    }
    if let Some(animated) = animated_building_model(kind, team) {
        building.insert(animated);
    }
    let entity = building.id();
    map.set_building(origin, spec.footprint, Some(entity));
    entity
}

/// Cell just outside the building's +X side, where units leave and harvesters dock.
pub fn exit_cell(building: &Building) -> IVec2 {
    let footprint = building.kind.spec().footprint;
    building.origin + IVec2::new(footprint.x, footprint.y / 2)
}
