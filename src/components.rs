use std::collections::VecDeque;

use bevy::prelude::*;

use crate::catalog::{ArmorClass, BuildingKind, UnitKind, WeaponSpec};

#[derive(Component)]
pub struct Health {
    pub current: f32,
    pub max: f32,
}

impl Health {
    pub fn full(max: f32) -> Self {
        Self { current: max, max }
    }

    pub fn fraction(&self) -> f32 {
        (self.current / self.max).clamp(0.0, 1.0)
    }
}

#[derive(Component)]
pub struct Unit(pub UnitKind);

#[derive(Component)]
pub struct Building {
    pub kind: BuildingKind,
    pub origin: IVec2,
}

#[derive(Component, Clone, Copy)]
pub struct BodyRadius(pub f32);

#[derive(Component, Clone, Copy)]
pub struct Armor(pub ArmorClass);

#[derive(Component)]
pub struct Selected;

#[derive(Component)]
pub struct Mobile {
    pub speed: f32,
    pub turn_rate: f32,
    pub facing: f32,
    pub path: VecDeque<Vec3>,
    /// True while following a player/AI move order, which auto-targeting must not interrupt.
    pub ordered_move: bool,
}

impl Mobile {
    pub fn new(speed: f32, turn_rate: f32, facing: f32) -> Self {
        Self {
            speed,
            turn_rate,
            facing,
            path: VecDeque::new(),
            ordered_move: false,
        }
    }

    pub fn is_moving(&self) -> bool {
        !self.path.is_empty()
    }

    pub fn stop(&mut self) {
        self.path.clear();
        self.ordered_move = false;
    }
}

#[derive(Component)]
pub struct Weapon {
    pub spec: WeaponSpec,
    pub cooldown: f32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TargetMode {
    Ordered,
    Automatic,
}

#[derive(Component, Default)]
pub struct AttackTarget {
    pub target: Option<(Entity, TargetMode)>,
    pub repath_timer: f32,
}

impl AttackTarget {
    pub fn entity(&self) -> Option<Entity> {
        self.target.map(|(entity, _)| entity)
    }

    pub fn order(&mut self, entity: Entity) {
        self.target = Some((entity, TargetMode::Ordered));
        self.repath_timer = 0.0;
    }

    pub fn clear(&mut self) {
        self.target = None;
    }
}

#[derive(Component)]
pub struct Turret {
    pub yaw: f32,
    pub turn_rate: f32,
    pub node: Option<Entity>,
    pub node_base_rotation: Quat,
}

impl Turret {
    pub fn new(yaw: f32, turn_rate: f32) -> Self {
        Self {
            yaw,
            turn_rate,
            node: None,
            node_base_rotation: Quat::IDENTITY,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum HarvestState {
    /// Waits for an explicit order; set when the player moves the harvester by hand.
    Holding,
    SeekingField,
    TravellingToField(IVec2),
    Mining {
        cell: IVec2,
        timer: f32,
    },
    ReturningToRefinery(Entity),
    Unloading {
        refinery: Entity,
        timer: f32,
    },
}

#[derive(Component)]
pub struct Harvester {
    pub state: HarvestState,
    pub cargo: u32,
    pub preferred_field: Option<IVec2>,
}

pub fn yaw_towards(from: Vec3, to: Vec3) -> f32 {
    let delta = to - from;
    (-delta.z).atan2(delta.x)
}

pub fn shortest_angle(from: f32, to: f32) -> f32 {
    let tau = std::f32::consts::TAU;
    let diff = (to - from).rem_euclid(tau);
    if diff > std::f32::consts::PI {
        diff - tau
    } else {
        diff
    }
}

pub fn rotate_towards(current: f32, target: f32, max_step: f32) -> f32 {
    let diff = shortest_angle(current, target);
    current + diff.clamp(-max_step, max_step)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::{FRAC_PI_2, PI};

    #[test]
    fn yaw_matches_the_model_facing_convention() {
        assert!(yaw_towards(Vec3::ZERO, Vec3::X).abs() < 1e-6);
        assert!((yaw_towards(Vec3::ZERO, Vec3::NEG_Z) - FRAC_PI_2).abs() < 1e-6);
        let facing =
            Quat::from_rotation_y(yaw_towards(Vec3::ZERO, Vec3::new(3.0, 0.0, -3.0))) * Vec3::X;
        assert!(facing.angle_between(Vec3::new(1.0, 0.0, -1.0)) < 1e-5);
    }

    #[test]
    fn turning_takes_the_short_way_and_never_overshoots() {
        assert!((shortest_angle(PI - 0.1, -PI + 0.1) - 0.2).abs() < 1e-5);
        assert!((rotate_towards(0.0, 1.0, 0.25) - 0.25).abs() < 1e-6);
        assert!((rotate_towards(0.0, 0.1, 0.25) - 0.1).abs() < 1e-6);
    }

    #[test]
    fn health_fraction_is_clamped() {
        let mut health = Health::full(200.0);
        health.current = -15.0;
        assert_eq!(health.fraction(), 0.0);
        health.current = 100.0;
        assert_eq!(health.fraction(), 0.5);
    }
}
