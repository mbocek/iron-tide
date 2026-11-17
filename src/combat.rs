use std::collections::HashMap;

use bevy::prelude::*;
use rand::RngExt;

use crate::animation::{animated_unit_model, spawn_corpse};
use crate::assets::ModelLibrary;
use crate::catalog::{ArmorClass, ProjectileStyle, Team, WeaponSpec};
use crate::components::*;
use crate::effects::{BlastSize, EffectAssets, spawn_blast};
use crate::fog::FogOfWar;
use crate::map::{GameMap, world_to_cell};
use crate::movement::order_move;
use crate::sound::{Sound, SoundCue};

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

#[derive(Clone, Copy)]
pub struct TargetInfo {
    pub entity: Entity,
    pub team: Team,
    pub position: Vec3,
    pub radius: f32,
    pub armor: ArmorClass,
    pub is_building: bool,
    /// Whether the player can currently see it; computer opponents ignore the fog.
    pub seen_by_player: bool,
}

/// Per-frame copy of every damageable entity, so systems can look up targets without query conflicts.
#[derive(Resource, Default)]
pub struct Targets {
    list: Vec<TargetInfo>,
    by_entity: HashMap<Entity, usize>,
}

impl Targets {
    pub fn get(&self, entity: Entity) -> Option<&TargetInfo> {
        self.by_entity.get(&entity).map(|&i| &self.list[i])
    }

    pub fn iter(&self) -> impl Iterator<Item = &TargetInfo> {
        self.list.iter()
    }

    pub fn nearest_enemy(&self, team: Team, from: Vec3, max_reach: f32) -> Option<&TargetInfo> {
        self.list
            .iter()
            .filter(|t| t.team.is_hostile_to(team) && (team != Team::Player || t.seen_by_player))
            .map(|t| (t, surface_distance(from, t)))
            .filter(|&(_, distance)| distance <= max_reach)
            .min_by(|a, b| {
                let rank = |(t, d): &(&TargetInfo, f32)| d + if t.is_building { 4.0 } else { 0.0 };
                rank(a).total_cmp(&rank(b))
            })
            .map(|(t, _)| t)
    }
}

pub fn surface_distance(from: Vec3, target: &TargetInfo) -> f32 {
    (from.xz().distance(target.position.xz()) - target.radius).max(0.0)
}

#[derive(Component)]
pub struct Projectile {
    team: Team,
    target: Option<Entity>,
    start: Vec3,
    end: Vec3,
    elapsed: f32,
    duration: f32,
    arc_height: f32,
    weapon: WeaponSpec,
}

pub fn snapshot_targets(
    mut targets: ResMut<Targets>,
    fog: Res<FogOfWar>,
    damageable: Query<
        (
            Entity,
            &Team,
            &Transform,
            &BodyRadius,
            &Armor,
            Has<Building>,
        ),
        With<Health>,
    >,
) {
    targets.list.clear();
    targets.by_entity.clear();
    for (entity, &team, transform, radius, armor, is_building) in &damageable {
        let index = targets.list.len();
        targets.by_entity.insert(entity, index);
        targets.list.push(TargetInfo {
            entity,
            team,
            position: transform.translation,
            radius: radius.0,
            armor: armor.0,
            is_building,
            seen_by_player: fog.reveals(team, transform.translation),
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
            let Some(target) = targets.get(entity) else {
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
        if let Some(enemy) = targets.nearest_enemy(team, position, reach) {
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
    time: Res<Time>,
    targets: Res<Targets>,
    effects: Res<EffectAssets>,
    mut shooters: Query<(
        &Team,
        &mut Transform,
        &mut Weapon,
        &AttackTarget,
        Option<&mut Turret>,
        Option<&mut Mobile>,
    )>,
) {
    let dt = time.delta_secs();
    for (&team, mut transform, mut weapon, attack, turret, mobile) in &mut shooters {
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
        weapon.cooldown = weapon.spec.reload_secs;
        let barrel_direction = Quat::from_rotation_y(aimed_yaw) * Vec3::X;
        let muzzle = position + Vec3::Y * MUZZLE_HEIGHT + barrel_direction * BARREL_LENGTH;
        fire_projectile(&mut commands, &effects, team, muzzle, target, weapon.spec);
        cues.write(SoundCue::at(weapon.spec.report, muzzle));
    }
}

fn fire_projectile(
    commands: &mut Commands,
    effects: &EffectAssets,
    team: Team,
    muzzle: Vec3,
    target: &TargetInfo,
    weapon: WeaponSpec,
) {
    let mut end = target.position + Vec3::Y * TARGET_CENTER_HEIGHT;
    let mut arc_height = 0.0;
    let mut homing_target = Some(target.entity);
    if weapon.style == ProjectileStyle::ArtilleryShell {
        let mut rng = rand::rng();
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

    let flash_size = if weapon.style == ProjectileStyle::Bullet {
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
    ));
}

pub fn fly_projectiles(
    mut cues: MessageWriter<SoundCue>,
    mut commands: Commands,
    time: Res<Time>,
    targets: Res<Targets>,
    effects: Res<EffectAssets>,
    mut projectiles: Query<(Entity, &mut Projectile, &mut Transform)>,
    mut healths: Query<&mut Health>,
    mut damage_log: MessageWriter<DamageDealt>,
) {
    for (entity, mut projectile, mut transform) in &mut projectiles {
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
                .filter(|t| t.team.is_hostile_to(projectile.team))
            {
                let distance = surface_distance(impact, victim);
                if distance > weapon.splash_radius {
                    continue;
                }
                let falloff = 1.0 - 0.6 * distance / weapon.splash_radius;
                apply_damage(
                    &mut healths,
                    &mut damage_log,
                    victim,
                    weapon.damage * falloff * weapon.multiplier_against(victim.armor),
                );
            }
        } else if let Some(victim) = projectile.target.and_then(|e| targets.get(e)) {
            apply_damage(
                &mut healths,
                &mut damage_log,
                victim,
                weapon.damage * weapon.multiplier_against(victim.armor),
            );
        }
        let blast = match weapon.style {
            ProjectileStyle::Bullet => BlastSize::BulletImpact,
            ProjectileStyle::Shell | ProjectileStyle::ArtilleryShell | ProjectileStyle::Missile => {
                BlastSize::ShellImpact
            }
        };
        spawn_blast(&mut commands, &effects, impact, blast);
        let impact_sound = match weapon.style {
            ProjectileStyle::Bullet => Sound::BulletImpact,
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
}

fn apply_damage(
    healths: &mut Query<&mut Health>,
    damage_log: &mut MessageWriter<DamageDealt>,
    victim: &TargetInfo,
    amount: f32,
) {
    if let Ok(mut health) = healths.get_mut(victim.entity) {
        health.current -= amount;
        damage_log.write(DamageDealt {
            victim: victim.entity,
            team: victim.team,
            amount,
        });
    }
}

pub fn remove_destroyed(
    mut cues: MessageWriter<SoundCue>,
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
    )>,
) {
    for (entity, health, transform, &team, building, unit) in &destroyed {
        if health.current > 0.0 {
            continue;
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
