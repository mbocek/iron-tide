use std::collections::HashMap;

use crate::abilities::Camouflage;
use bevy::prelude::*;
use rand::RngExt;

use crate::animation::{animated_unit_model, spawn_corpse};
use crate::assets::ModelLibrary;
use crate::catalog::{
    ArmorClass, BuildingKind, Diplomacy, ProjectileStyle, Team, UnitKind, WeaponSpec,
};
use crate::components::*;
use crate::determinism::{SimId, SimulationRng, in_simulation_order};
use crate::effects::{BlastSize, EffectAssets, spawn_blast};
use crate::fog::FogOfWar;
use crate::intel::Intel;
use crate::interpolation::TickTransforms;
use crate::map::{CELL_SIZE, GameMap, world_to_cell};
use crate::movement::order_move;
use crate::player_commands::LocalTeam;
use crate::production::Production;
use crate::research::{incoming_damage_factor, outgoing_damage_factor, reload_factor};
use crate::sim_math;
use crate::sound::{Sound, SoundCue};
use crate::veterancy::{Rank, Veterancy};

const SIGHT_BEYOND_RANGE: f32 = 6.0;
const CHASE_REPATH_SECS: f32 = 1.0;
const AIM_TOLERANCE: f32 = 0.12;
const BODY_AIM_TURN_RATE: f32 = 6.0;
const ARTILLERY_SCATTER: f32 = 1.2;
const ARTILLERY_ARC_PER_METER: f32 = 0.25;
const MISSILE_ARC_PER_METER: f32 = 0.07;
const BARREL_LENGTH: f32 = 1.4;
const MUZZLE_HEIGHT: f32 = 1.2;
const TARGET_CENTER_HEIGHT: f32 = 0.7;
const SHELL_SCORCH_RADIUS: f32 = 0.8;
const VEHICLE_SCORCH_RADIUS: f32 = 1.8;

#[derive(Clone, Copy)]
pub struct TargetInfo {
    pub entity: Entity,
    pub team: Team,
    pub position: Vec3,
    pub radius: f32,
    pub armor: ArmorClass,
    pub is_building: bool,
    pub health_fraction: f32,
    /// Whether the local player may see it; only the interface reads this.
    pub seen_locally: bool,
    /// Indexed by `Team::index`: whether that team can currently see it.
    seen_by_team: [bool; 4],
}

impl TargetInfo {
    pub fn seen_by(&self, viewer: Team) -> bool {
        self.seen_by_team[viewer.index()]
    }
}

/// Per-frame copy of every damageable entity, so systems can look up targets without query conflicts.
#[derive(Resource, Default)]
pub struct Targets {
    list: Vec<TargetInfo>,
    by_entity: HashMap<Entity, usize>,
    diplomacy: Diplomacy,
}

impl Targets {
    pub fn get(&self, entity: Entity) -> Option<&TargetInfo> {
        self.by_entity.get(&entity).map(|&i| &self.list[i])
    }

    pub fn iter(&self) -> impl Iterator<Item = &TargetInfo> {
        self.list.iter()
    }

    pub fn hostile(&self, team: Team, other: Team) -> bool {
        self.diplomacy.hostile(team, other)
    }

    /// The enemy this weapon should fire at: one it hurts most and that is nearly finished, preferably
    /// already in range; buildings come after anything that shoots back.
    pub fn best_enemy(
        &self,
        team: Team,
        weapon: &WeaponSpec,
        from: Vec3,
        max_reach: f32,
    ) -> Option<&TargetInfo> {
        let worth = |target: &TargetInfo, distance: f32| {
            let effectiveness = weapon.multiplier_against(target.armor).min(2.0);
            let out_of_range = if distance > weapon.range {
                OUT_OF_RANGE_PENALTY
            } else {
                0.0
            };
            let structure = if target.is_building {
                STRUCTURE_PENALTY
            } else {
                0.0
            };
            distance + out_of_range + structure
                - EFFECTIVENESS_PULL * effectiveness
                - WOUNDED_PULL * (1.0 - target.health_fraction)
        };
        self.list
            .iter()
            .filter(|t| self.hostile(t.team, team) && t.seen_by(team) && weapon.can_hit(t.armor))
            .map(|t| (t, surface_distance(from, t)))
            .filter(|&(_, distance)| distance <= max_reach)
            .min_by(|(a, da), (b, db)| worth(a, *da).total_cmp(&worth(b, *db)))
            .map(|(t, _)| t)
    }
}

/// Metres a target out of reach counts as farther away, so units shoot what they can before chasing.
const OUT_OF_RANGE_PENALTY: f32 = 6.0;
const STRUCTURE_PENALTY: f32 = 4.0;
/// Metres a target counts as nearer per unit of the weapon's damage multiplier against it.
const EFFECTIVENESS_PULL: f32 = 4.0;
/// Metres a nearly dead target counts as nearer than an unhurt one.
const WOUNDED_PULL: f32 = 4.0;

pub fn surface_distance(from: Vec3, target: &TargetInfo) -> f32 {
    (from.xz().distance(target.position.xz()) - target.radius).max(0.0)
}

/// Who fired a shot, for balance statistics.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum DamageSource {
    Unit(UnitKind),
    Building(BuildingKind),
}

/// Sent when a unit or structure is destroyed.
#[derive(Message, Clone, Copy, Debug)]
pub struct Destroyed {
    pub entity: Entity,
    pub team: Team,
    pub what: DamageSource,
    pub position: Vec3,
}

/// An explosion that leaves a scorch mark of this radius in meters on the ground.
#[derive(Message, Clone, Copy, Debug)]
pub struct Impact {
    pub position: Vec3,
    pub scorch_radius: f32,
}

#[derive(Component)]
pub struct Projectile {
    team: Team,
    source: DamageSource,
    shooter: Entity,
    target: Option<Entity>,
    start: Vec3,
    end: Vec3,
    elapsed: f32,
    duration: f32,
    arc_height: f32,
    weapon: WeaponSpec,
}

impl Projectile {
    /// Start, end, arc height, style and flight time: enough to show the shot again in a replay.
    pub fn flight(&self) -> (Vec3, Vec3, f32, ProjectileStyle, f32) {
        (
            self.start,
            self.end,
            self.arc_height,
            self.weapon.style,
            self.duration,
        )
    }
}

pub fn snapshot_targets(
    local: Res<LocalTeam>,
    mut targets: ResMut<Targets>,
    fog: Res<FogOfWar>,
    intel: Res<Intel>,
    diplomacy: Res<Diplomacy>,
    damageable: Query<(
        &SimId,
        Entity,
        &Team,
        &Transform,
        &BodyRadius,
        &Armor,
        Has<Building>,
        Option<&Camouflage>,
        &Health,
    )>,
) {
    targets.list.clear();
    targets.by_entity.clear();
    targets.diplomacy = *diplomacy;
    let mut in_simulation_order: Vec<_> = damageable.iter().collect();
    in_simulation_order.sort_by_key(|target| *target.0);
    for (_, entity, &team, transform, radius, armor, is_building, camouflage, health) in
        in_simulation_order
    {
        let concealed = camouflage.is_some_and(|camouflage| camouflage.concealed);
        let index = targets.list.len();
        targets.by_entity.insert(entity, index);
        targets.list.push(TargetInfo {
            entity,
            team,
            position: transform.translation,
            radius: radius.0,
            armor: armor.0,
            is_building,
            health_fraction: health.fraction(),
            seen_locally: !diplomacy.hostile(team, local.0)
                || (!concealed
                    && intel
                        .vision(local.0, &fog)
                        .is_visible(transform.translation)),
            seen_by_team: Team::ALL.map(|viewer| {
                !diplomacy.hostile(team, viewer)
                    || (!concealed && intel.vision(viewer, &fog).is_visible(transform.translation))
            }),
        });
    }
}

pub fn acquire_targets(
    targets: Res<Targets>,
    mut shooters: Query<(
        &Team,
        &Transform,
        &Weapon,
        &mut AttackTarget,
        Option<&Mobile>,
        Has<Turret>,
    )>,
) {
    for (&team, transform, weapon, mut attack, mobile, has_turret) in &mut shooters {
        let position = transform.translation;
        let sight = weapon.spec.range + SIGHT_BEYOND_RANGE;
        if let Some((entity, mode)) = attack.target {
            let Some(target) = targets
                .get(entity)
                .filter(|target| weapon.spec.can_hit(target.armor))
            else {
                attack.clear();
                continue;
            };
            let lost_sight =
                mode == TargetMode::Automatic && surface_distance(position, target) > sight;
            if !lost_sight {
                continue;
            }
            attack.clear();
        }

        let on_ordered_move = mobile.is_some_and(|m| m.ordered_move && m.is_moving());
        if on_ordered_move && !has_turret {
            continue;
        }
        let can_chase = mobile.is_some() && !on_ordered_move;
        let reach = if can_chase { sight } else { weapon.spec.range };
        if let Some(enemy) = targets.best_enemy(team, &weapon.spec, position, reach) {
            attack.target = Some((enemy.entity, TargetMode::Automatic));
            attack.repath_timer = 0.0;
        }
    }
}

pub fn chase_targets(
    time: Res<Time>,
    map: Res<GameMap>,
    targets: Res<Targets>,
    mut chasers: Query<(&Transform, &Weapon, &mut AttackTarget, &mut Mobile)>,
) {
    for (transform, weapon, mut attack, mut mobile) in &mut chasers {
        let Some((entity, mode)) = attack.target else {
            continue;
        };
        let Some(target) = targets.get(entity) else {
            continue;
        };
        if mode == TargetMode::Automatic && mobile.ordered_move {
            continue;
        }
        let in_range = surface_distance(transform.translation, target) <= weapon.spec.range;
        if in_range {
            if mobile.is_moving() {
                mobile.stop();
            }
            continue;
        }
        attack.repath_timer -= time.delta_secs();
        if attack.repath_timer <= 0.0 {
            attack.repath_timer = CHASE_REPATH_SECS;
            order_move(
                &mut mobile,
                &map,
                transform.translation,
                world_to_cell(target.position),
                false,
            );
        }
    }
}

pub fn aim_and_fire(
    mut cues: MessageWriter<SoundCue>,
    mut commands: Commands,
    production: Res<Production>,
    mut rng: ResMut<SimulationRng>,
    time: Res<Time>,
    targets: Res<Targets>,
    effects: Res<EffectAssets>,
    mut shooters: Query<(
        Entity,
        &Team,
        &mut Transform,
        &mut Weapon,
        &AttackTarget,
        Option<&mut Turret>,
        Option<&mut Mobile>,
        Option<&Unit>,
        Option<&Building>,
        Option<&Veterancy>,
        Option<&mut Camouflage>,
        &SimId,
    )>,
) {
    let dt = time.delta_secs();
    for (
        shooter,
        &team,
        mut transform,
        mut weapon,
        attack,
        turret,
        mobile,
        unit,
        building,
        veterancy,
        camouflage,
        _,
    ) in in_simulation_order(shooters.iter_mut(), |shooter| *shooter.11)
    {
        let source = match (unit, building) {
            (Some(unit), _) => DamageSource::Unit(unit.0),
            (None, Some(building)) => DamageSource::Building(building.kind),
            (None, None) => continue,
        };
        weapon.cooldown = (weapon.cooldown - dt).max(0.0);
        let body_yaw = mobile.as_ref().map_or(0.0, |m| m.facing);
        let target = attack.entity().and_then(|entity| targets.get(entity));

        let Some(target) = target else {
            if let Some(mut turret) = turret {
                turret.yaw = rotate_towards(turret.yaw, body_yaw, turret.turn_rate * dt);
            }
            continue;
        };

        let position = transform.translation;
        let desired_yaw = yaw_towards(position, target.position);
        let aimed_yaw = match (turret, mobile) {
            (Some(mut turret), _) => {
                turret.yaw = rotate_towards(turret.yaw, desired_yaw, turret.turn_rate * dt);
                turret.yaw
            }
            (None, Some(mut mobile)) if !mobile.is_moving() => {
                mobile.facing = rotate_towards(mobile.facing, desired_yaw, BODY_AIM_TURN_RATE * dt);
                transform.rotation = Quat::from_rotation_y(mobile.facing);
                mobile.facing
            }
            (None, Some(_)) => continue,
            (None, None) => desired_yaw,
        };

        let in_range = surface_distance(position, target) <= weapon.spec.range;
        let aimed = shortest_angle(aimed_yaw, desired_yaw).abs() < AIM_TOLERANCE;
        if !(in_range && aimed && weapon.cooldown <= 0.0) {
            continue;
        }
        let shooter_armor = unit.map(|unit| unit.0.spec().armor);
        weapon.cooldown = weapon.spec.reload_secs * reload_factor(&production, team, shooter_armor);
        if let Some(mut camouflage) = camouflage {
            camouflage.betray_by_firing();
        }
        let barrel_direction = sim_math::facing_direction(aimed_yaw);
        let muzzle = position + Vec3::Y * MUZZLE_HEIGHT + barrel_direction * BARREL_LENGTH;
        let rank = veterancy.map_or(Rank::Recruit, |veterancy| veterancy.rank);
        let mut ranked_weapon = weapon.spec;
        ranked_weapon.damage *= rank.damage_factor() * outgoing_damage_factor(&production, team);
        let shot = Shot {
            team,
            source,
            shooter,
            muzzle,
            weapon: ranked_weapon,
        };
        fire_projectile(&mut commands, &mut rng, &effects, shot, target);
        cues.write(SoundCue::at(weapon.spec.report, muzzle));
    }
}

/// Who fires what from where; the weapon's damage already includes the shooter's rank.
pub(crate) struct Shot {
    pub team: Team,
    pub source: DamageSource,
    pub shooter: Entity,
    pub muzzle: Vec3,
    pub weapon: WeaponSpec,
}

pub(crate) fn fire_projectile(
    commands: &mut Commands,
    rng: &mut SimulationRng,
    effects: &EffectAssets,
    shot: Shot,
    target: &TargetInfo,
) {
    let Shot {
        team,
        source,
        shooter,
        muzzle,
        weapon,
    } = shot;
    let mut end = target.position + Vec3::Y * TARGET_CENTER_HEIGHT;
    let mut arc_height = 0.0;
    let mut homing_target = Some(target.entity);
    if weapon.style == ProjectileStyle::ArtilleryShell {
        end += Vec3::new(
            rng.random_range(-ARTILLERY_SCATTER..ARTILLERY_SCATTER),
            -TARGET_CENTER_HEIGHT,
            rng.random_range(-ARTILLERY_SCATTER..ARTILLERY_SCATTER),
        );
        arc_height = muzzle.distance(end) * ARTILLERY_ARC_PER_METER;
        homing_target = None;
    }
    if weapon.style == ProjectileStyle::Missile {
        arc_height = muzzle.distance(end) * MISSILE_ARC_PER_METER;
    }

    let flash_size = if matches!(
        weapon.style,
        ProjectileStyle::Bullet | ProjectileStyle::Flame
    ) {
        0.0
    } else {
        1.0
    };
    if flash_size > 0.0 {
        spawn_blast(commands, effects, muzzle, BlastSize::MuzzleFlash);
    }
    commands.spawn((
        Projectile {
            team,
            source,
            shooter,
            target: homing_target,
            start: muzzle,
            end,
            elapsed: 0.0,
            duration: (muzzle.distance(end) / weapon.projectile_speed).max(0.05),
            arc_height,
            weapon,
        },
        effects.projectile_visual(weapon.style),
        Transform::from_translation(muzzle),
        TickTransforms::default(),
    ));
}

pub fn fly_projectiles(
    mut cues: MessageWriter<SoundCue>,
    mut commands: Commands,
    production: Res<Production>,
    time: Res<Time>,
    targets: Res<Targets>,
    effects: Res<EffectAssets>,
    mut projectiles: Query<(Entity, &mut Projectile, &mut Transform, &SimId)>,
    mut healths: Query<&mut Health>,
    mut damage_log: MessageWriter<DamageDealt>,
    mut impacts: MessageWriter<Impact>,
) {
    let in_order = in_simulation_order(projectiles.iter_mut(), |item| *item.3);
    for (entity, mut projectile, mut transform, _) in in_order {
        if let Some(target) = projectile.target.and_then(|e| targets.get(e)) {
            projectile.end = target.position + Vec3::Y * TARGET_CENTER_HEIGHT;
        }
        projectile.elapsed += time.delta_secs();
        let t = (projectile.elapsed / projectile.duration).min(1.0);
        let previous = transform.translation;
        let arc = 4.0 * projectile.arc_height * t * (1.0 - t);
        transform.translation = projectile.start.lerp(projectile.end, t) + Vec3::Y * arc;
        let heading = transform.translation - previous;
        if heading.length_squared() > 1e-6 {
            transform.rotation = Quat::from_rotation_arc(Vec3::Y, heading.normalize());
        }
        if t < 1.0 {
            continue;
        }

        commands.entity(entity).despawn();
        let impact = projectile.end;
        let weapon = projectile.weapon;
        if weapon.splash_radius > 0.0 {
            for victim in targets
                .iter()
                .filter(|t| targets.hostile(t.team, projectile.team))
            {
                let distance = surface_distance(impact, victim);
                if distance > weapon.splash_radius {
                    continue;
                }
                let falloff = 1.0 - 0.6 * distance / weapon.splash_radius;
                apply_damage(
                    &mut healths,
                    &mut damage_log,
                    &production,
                    victim,
                    weapon.damage * falloff * weapon.multiplier_against(victim.armor),
                    &projectile,
                );
            }
        } else if let Some(victim) = projectile.target.and_then(|e| targets.get(e)) {
            apply_damage(
                &mut healths,
                &mut damage_log,
                &production,
                victim,
                weapon.damage * weapon.multiplier_against(victim.armor),
                &projectile,
            );
        }
        let blast = match weapon.style {
            ProjectileStyle::Bullet | ProjectileStyle::Flame => BlastSize::BulletImpact,
            ProjectileStyle::Shell | ProjectileStyle::ArtilleryShell | ProjectileStyle::Missile => {
                BlastSize::ShellImpact
            }
        };
        spawn_blast(&mut commands, &effects, impact, blast);
        let scorch_radius = match weapon.style {
            ProjectileStyle::Bullet => 0.0,
            ProjectileStyle::Shell | ProjectileStyle::Missile | ProjectileStyle::Flame => {
                SHELL_SCORCH_RADIUS
            }
            ProjectileStyle::ArtilleryShell => weapon.splash_radius * 0.8,
        };
        if scorch_radius > 0.0 {
            impacts.write(Impact {
                position: impact,
                scorch_radius,
            });
        }
        let impact_sound = match weapon.style {
            ProjectileStyle::Bullet | ProjectileStyle::Flame => Sound::BulletImpact,
            ProjectileStyle::Shell | ProjectileStyle::ArtilleryShell | ProjectileStyle::Missile => {
                Sound::ShellExplosion
            }
        };
        cues.write(SoundCue::at(impact_sound, impact));
    }
}

/// Emitted for every hit so the interface can show how much damage landed.
#[derive(Message, Clone, Copy)]
pub struct DamageDealt {
    pub victim: Entity,
    pub team: Team,
    pub amount: f32,
    pub source: DamageSource,
    pub attacker: Team,
    pub shooter: Entity,
}

fn apply_damage(
    healths: &mut Query<&mut Health>,
    damage_log: &mut MessageWriter<DamageDealt>,
    production: &Production,
    victim: &TargetInfo,
    amount: f32,
    projectile: &Projectile,
) {
    let amount = amount * incoming_damage_factor(production, victim.team, victim.armor);
    if let Ok(mut health) = healths.get_mut(victim.entity) {
        health.current -= amount;
        damage_log.write(DamageDealt {
            victim: victim.entity,
            team: victim.team,
            amount,
            source: projectile.source,
            attacker: projectile.team,
            shooter: projectile.shooter,
        });
    }
}

pub fn remove_destroyed(
    mut cues: MessageWriter<SoundCue>,
    mut losses: MessageWriter<Destroyed>,
    mut impacts: MessageWriter<Impact>,
    mut commands: Commands,
    effects: Res<EffectAssets>,
    models: Res<ModelLibrary>,
    mut map: ResMut<GameMap>,
    destroyed: Query<(
        Entity,
        &Health,
        &Transform,
        &Team,
        Option<&Building>,
        Option<&Unit>,
        &SimId,
    )>,
) {
    let in_order = in_simulation_order(destroyed, |item| *item.6);
    for (entity, health, transform, &team, building, unit, _) in in_order {
        if health.current > 0.0 {
            continue;
        }
        let what = match (unit, building) {
            (Some(unit), _) => Some(DamageSource::Unit(unit.0)),
            (None, Some(building)) => Some(DamageSource::Building(building.kind)),
            (None, None) => None,
        };
        if let Some(what) = what {
            losses.write(Destroyed {
                entity,
                team,
                what,
                position: transform.translation,
            });
        }
        let blast = match (building, unit) {
            (Some(building), _) => {
                map.set_building(building.origin, building.kind.spec().footprint, None);
                BlastSize::BuildingCollapse
            }
            (None, Some(unit)) if unit.0.spec().armor == ArmorClass::Infantry => {
                spawn_corpse(
                    &mut commands,
                    models.unit(unit.0, team),
                    animated_unit_model(unit.0, team),
                    *transform,
                );
                BlastSize::BulletImpact
            }
            _ => BlastSize::VehicleWreck,
        };
        let scorch_radius = match (blast, building) {
            (BlastSize::BuildingCollapse, Some(building)) => {
                building.kind.spec().footprint.as_vec2().max_element() * CELL_SIZE * 0.6
            }
            (BlastSize::VehicleWreck, _) => VEHICLE_SCORCH_RADIUS,
            _ => 0.0,
        };
        if scorch_radius > 0.0 {
            impacts.write(Impact {
                position: transform.translation,
                scorch_radius,
            });
        }
        let destruction_sound = match blast {
            BlastSize::BuildingCollapse => Some(Sound::BuildingCollapse),
            BlastSize::VehicleWreck => Some(Sound::VehicleExplosion),
            _ => None,
        };
        if let Some(sound) = destruction_sound {
            cues.write(SoundCue::at(sound, transform.translation));
        }
        spawn_blast(
            &mut commands,
            &effects,
            transform.translation + Vec3::Y * 0.5,
            blast,
        );
        commands.entity(entity).despawn();
    }
}

pub fn link_turret_nodes(
    mut turrets: Query<(Entity, &mut Turret)>,
    children: Query<&Children>,
    nodes: Query<(&Name, &Transform)>,
) {
    for (entity, mut turret) in &mut turrets {
        if turret.node.is_some() {
            continue;
        }
        let found = children.iter_descendants(entity).find_map(|node| {
            nodes
                .get(node)
                .ok()
                .filter(|(name, _)| name.as_str() == "Turret")
                .map(|(_, t)| (node, t.rotation))
        });
        if let Some((node, base_rotation)) = found {
            turret.node = Some(node);
            turret.node_base_rotation = base_rotation;
        }
    }
}

pub fn rotate_turret_nodes(
    turrets: Query<(&Turret, Option<&Mobile>)>,
    mut nodes: Query<&mut Transform, Without<Turret>>,
) {
    for (turret, mobile) in &turrets {
        let Some(mut node_transform) = turret.node.and_then(|node| nodes.get_mut(node).ok()) else {
            continue;
        };
        let body_yaw = mobile.map_or(0.0, |m| m.facing);
        node_transform.rotation =
            Quat::from_rotation_y(turret.yaw - body_yaw) * turret.node_base_rotation;
    }
}
