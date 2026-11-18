use std::collections::HashMap;
use std::time::Duration;

use bevy::animation::RepeatAnimation;
use bevy::gltf::Gltf;
use bevy::prelude::*;
use rand::RngExt;

use crate::camera::RtsCamera;
use crate::catalog::{ArmorClass, BuildingKind, Team, UnitKind};
use crate::combat::{Targets, surface_distance};
use crate::components::*;
use crate::map::CrystalVisual;
use crate::medical::Healer;

/// Clip names authored in Blender; tools/blender/irontide_models exports them with exactly these names.
const IDLE: &str = "Idle";
const WALK: &str = "Walk";
const KNEEL_FIRE: &str = "KneelFire";
const DEATH: &str = "Death";
const DRIVE: &str = "Drive";
const FIRE: &str = "Fire";
const WORK: &str = "Work";

const BLEND_SECS: f32 = 0.2;
const HARVESTER_WORK_RATE: f32 = 1.0;
const CORPSE_LINGER_SECS: f32 = 7.0;
const CORPSE_SINK_SECS: f32 = 1.5;
const CORPSE_SINK_DEPTH: f32 = 0.6;
const CRYSTAL_MODELS: [&str; 3] = ["crystal_a", "crystal_b", "crystal_c"];

/// Identifies which model file's animations an entity plays (e.g. `tank_red`).
#[derive(Component, Clone)]
pub struct AnimatedModel(pub String);

#[derive(Component)]
pub struct AnimationLink {
    player: Entity,
    current: Option<&'static str>,
    last_cooldown: f32,
}

#[derive(Component)]
pub struct Corpse {
    age: f32,
}

struct ModelClips {
    graph: Handle<AnimationGraph>,
    clips: HashMap<String, AnimationNodeIndex>,
}

#[derive(Resource, Default)]
pub struct AnimationLibrary {
    pending: HashMap<String, Handle<Gltf>>,
    ready: HashMap<String, ModelClips>,
}

impl AnimationLibrary {
    fn clip(&self, model: &str, name: &str) -> Option<AnimationNodeIndex> {
        self.ready.get(model)?.clips.get(name).copied()
    }
}

/// Ground speed (m/s) at which each model's `Drive` loop was authored, so wheels turn without slipping.
fn authored_drive_speed(kind: UnitKind) -> f32 {
    match kind {
        UnitKind::Tank => 3.0,
        UnitKind::Artillery => 2.5,
        UnitKind::Scout => 5.0,
        UnitKind::Harvester => 2.0,
        UnitKind::Rifleman
        | UnitKind::Rocketeer
        | UnitKind::Medic
        | UnitKind::Engineer
        | UnitKind::Sniper
        | UnitKind::FlameTrooper => 2.4,
        UnitKind::Bulwark => 2.2,
        UnitKind::Gunship => 6.5,
        UnitKind::FlakTrack => 3.2,
        UnitKind::MissileTruck => 4.0,
    }
}

pub fn animated_unit_model(kind: UnitKind, team: Team) -> AnimatedModel {
    AnimatedModel(format!("{}_{}", kind.spec().model_id, team.model_variant()))
}

pub fn animated_building_model(kind: BuildingKind, team: Team) -> Option<AnimatedModel> {
    kind.spec()
        .weapon
        .is_some()
        .then(|| AnimatedModel(format!("{}_{}", kind.spec().model_id, team.model_variant())))
}

pub fn load_animation_sources(
    mut library: ResMut<AnimationLibrary>,
    asset_server: Res<AssetServer>,
) {
    let unit_models = UnitKind::INFANTRY
        .iter()
        .chain(UnitKind::VEHICLES.iter())
        .flat_map(|&kind| Team::ALL.map(|team| animated_unit_model(kind, team).0));
    let turret_models = BuildingKind::CONSTRUCTIBLE.into_iter().flat_map(|kind| {
        Team::ALL
            .into_iter()
            .filter_map(move |team| animated_building_model(kind, team).map(|m| m.0))
    });
    let crystal_models = CRYSTAL_MODELS.iter().map(|id| id.to_string());
    for model in unit_models.chain(turret_models).chain(crystal_models) {
        let handle = asset_server.load(format!("models/{model}.glb"));
        library.pending.insert(model, handle);
    }
}

pub fn build_animation_graphs(
    mut library: ResMut<AnimationLibrary>,
    gltfs: Res<Assets<Gltf>>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
) {
    let loaded: Vec<String> = library
        .pending
        .iter()
        .filter(|(_, handle)| gltfs.contains(*handle))
        .map(|(model, _)| model.clone())
        .collect();
    for model in loaded {
        let handle = library
            .pending
            .remove(&model)
            .expect("collected from pending");
        let gltf = gltfs.get(&handle).expect("checked as loaded");
        let names: Vec<String> = gltf
            .named_animations
            .keys()
            .map(|name| name.to_string())
            .collect();
        let (graph, indices) = AnimationGraph::from_clips(
            names
                .iter()
                .map(|name| gltf.named_animations[name.as_str()].clone()),
        );
        let clips = names.into_iter().zip(indices).collect();
        library.ready.insert(
            model,
            ModelClips {
                graph: graphs.add(graph),
                clips,
            },
        );
    }
}

/// Hooks the glTF scene's animation player up to the model's clip graph once the scene has spawned.
pub fn attach_animation_players(
    mut commands: Commands,
    library: Res<AnimationLibrary>,
    unlinked: Query<(Entity, &AnimatedModel), Without<AnimationLink>>,
    children: Query<&Children>,
    players: Query<(), With<AnimationPlayer>>,
) {
    for (entity, model) in &unlinked {
        let Some(clips) = library.ready.get(&model.0) else {
            continue;
        };
        let Some(player) = children
            .iter_descendants(entity)
            .find(|&node| players.contains(node))
        else {
            continue;
        };
        commands.entity(player).insert((
            AnimationGraphHandle(clips.graph.clone()),
            AnimationTransitions::new(),
        ));
        commands.entity(entity).insert(AnimationLink {
            player,
            current: None,
            last_cooldown: 0.0,
        });
    }
}

fn switch_to(
    link: &mut AnimationLink,
    library: &AnimationLibrary,
    model: &str,
    player: &mut AnimationPlayer,
    transitions: &mut AnimationTransitions,
    clip: &'static str,
    repeat: bool,
) {
    if link.current == Some(clip) {
        return;
    }
    let Some(index) = library.clip(model, clip) else {
        return;
    };
    let active = transitions.play(player, index, Duration::from_secs_f32(BLEND_SECS));
    if repeat {
        active.repeat();
    } else {
        active.set_repeat(RepeatAnimation::Never);
    }
    link.current = Some(clip);
}

pub fn animate_soldiers(
    library: Res<AnimationLibrary>,
    targets: Res<Targets>,
    mut soldiers: Query<
        (
            &AnimatedModel,
            &mut AnimationLink,
            &Transform,
            &Mobile,
            &Armor,
            Option<&AttackTarget>,
            Option<&Weapon>,
            Option<&Healer>,
        ),
        Without<Corpse>,
    >,
    mut players: Query<(&mut AnimationPlayer, &mut AnimationTransitions)>,
) {
    for (model, mut link, transform, mobile, armor, attack, weapon, healer) in &mut soldiers {
        if armor.0 != ArmorClass::Infantry {
            continue;
        }
        let Ok((mut player, mut transitions)) = players.get_mut(link.player) else {
            continue;
        };
        let engaging = attack
            .and_then(|a| a.entity())
            .and_then(|target| targets.get(target))
            .zip(weapon)
            .is_some_and(|(target, weapon)| {
                surface_distance(transform.translation, target) <= weapon.spec.range
            });
        let treating = healer.is_some_and(|h| h.treating);
        let clip = if mobile.is_moving() {
            WALK
        } else if engaging || treating {
            KNEEL_FIRE
        } else {
            IDLE
        };
        switch_to(
            &mut link,
            &library,
            &model.0,
            &mut player,
            &mut transitions,
            clip,
            true,
        );
    }
}

pub fn animate_vehicles(
    library: Res<AnimationLibrary>,
    mut vehicles: Query<(
        &AnimatedModel,
        &mut AnimationLink,
        Option<&Unit>,
        Option<&Mobile>,
        Option<&Weapon>,
        Option<&Harvester>,
        Option<&Armor>,
    )>,
    mut players: Query<(&mut AnimationPlayer, &mut AnimationTransitions)>,
) {
    for (model, mut link, unit, mobile, weapon, harvester, armor) in &mut vehicles {
        if armor.is_some_and(|a| a.0 == ArmorClass::Infantry)
            || mobile.is_none() && weapon.is_none()
        {
            continue;
        }
        let Ok((mut player, mut transitions)) = players.get_mut(link.player) else {
            continue;
        };

        let mining = harvester.is_some_and(|h| matches!(h.state, HarvestState::Mining { .. }));
        let moving = mobile.is_some_and(|m| m.is_moving());
        if mobile.is_some() {
            let clip = if mining { WORK } else { DRIVE };
            switch_to(
                &mut link,
                &library,
                &model.0,
                &mut player,
                &mut transitions,
                clip,
                true,
            );
            if let Some(index) = library.clip(&model.0, clip)
                && let Some(active) = player.animation_mut(index)
            {
                let wheel_rate = unit.map_or(1.0, |u| u.0.spec().speed / authored_drive_speed(u.0));
                active.set_speed(match (mining, moving) {
                    (true, _) => HARVESTER_WORK_RATE,
                    (false, true) => wheel_rate,
                    (false, false) => 0.0,
                });
            }
        }

        let Some(weapon) = weapon else { continue };
        let just_fired = weapon.cooldown > link.last_cooldown + f32::EPSILON;
        link.last_cooldown = weapon.cooldown;
        if just_fired && let Some(fire) = library.clip(&model.0, FIRE) {
            player.play(fire).replay();
        }
    }
}

/// A crystal far enough from the camera focus to be off screen; its shards hold still meanwhile.
#[derive(Component)]
pub struct RestingCrystal;

const CRYSTAL_VIEW_CHECK_SECS: f32 = 0.5;
/// How far from the focus crystals may still be on screen, per meter of camera distance, plus a margin.
const CRYSTAL_VIEW_REACH_PER_METER: f32 = 1.0;
const CRYSTAL_VIEW_MARGIN: f32 = 15.0;

/// Every crystal's shards are animated transforms, so animating only those in view saves most of the work.
pub fn rest_offscreen_crystals(
    mut commands: Commands,
    time: Res<Time>,
    mut countdown: Local<f32>,
    cameras: Query<&RtsCamera>,
    mut crystals: Query<
        (Entity, &Transform, &mut AnimationLink, Has<RestingCrystal>),
        With<CrystalVisual>,
    >,
    mut players: Query<&mut AnimationPlayer>,
) {
    *countdown -= time.delta_secs();
    if *countdown > 0.0 {
        return;
    }
    *countdown = CRYSTAL_VIEW_CHECK_SECS;
    let Ok(camera) = cameras.single() else { return };
    let reach = camera.distance * CRYSTAL_VIEW_REACH_PER_METER + CRYSTAL_VIEW_MARGIN;
    for (crystal, transform, mut link, resting) in &mut crystals {
        let in_view = transform.translation.xz().distance(camera.focus.xz()) < reach;
        if in_view && resting {
            commands.entity(crystal).remove::<RestingCrystal>();
        } else if !in_view && !resting {
            if let Ok(mut player) = players.get_mut(link.player) {
                player.stop_all();
            }
            link.current = None;
            commands.entity(crystal).insert(RestingCrystal);
        }
    }
}

pub fn animate_crystals(
    library: Res<AnimationLibrary>,
    mut crystals: Query<
        (&AnimatedModel, &mut AnimationLink),
        (Without<Mobile>, Without<RestingCrystal>),
    >,
    mut players: Query<(&mut AnimationPlayer, &mut AnimationTransitions)>,
) {
    let mut rng = rand::rng();
    for (model, mut link) in &mut crystals {
        if link.current.is_some() || !CRYSTAL_MODELS.contains(&model.0.as_str()) {
            continue;
        }
        let Ok((mut player, mut transitions)) = players.get_mut(link.player) else {
            continue;
        };
        switch_to(
            &mut link,
            &library,
            &model.0,
            &mut player,
            &mut transitions,
            IDLE,
            true,
        );
        if let Some(index) = library.clip(&model.0, IDLE)
            && let Some(active) = player.animation_mut(index)
        {
            active.seek_to(rng.random_range(0.0..4.0));
        }
    }
}

pub fn spawn_corpse(
    commands: &mut Commands,
    model_scene: Handle<WorldAsset>,
    model: AnimatedModel,
    transform: Transform,
) {
    commands.spawn((
        Corpse { age: 0.0 },
        model,
        WorldAssetRoot(model_scene),
        transform,
    ));
}

/// Plays the fall once, then lets the body lie and finally sink into the ground.
pub fn animate_corpses(
    mut commands: Commands,
    time: Res<Time>,
    library: Res<AnimationLibrary>,
    mut corpses: Query<(
        Entity,
        &mut Corpse,
        &AnimatedModel,
        Option<&mut AnimationLink>,
        &mut Transform,
    )>,
    mut players: Query<(&mut AnimationPlayer, &mut AnimationTransitions)>,
) {
    for (entity, mut corpse, model, link, mut transform) in &mut corpses {
        corpse.age += time.delta_secs();
        if let Some(mut link) = link
            && let Ok((mut player, mut transitions)) = players.get_mut(link.player)
        {
            switch_to(
                &mut link,
                &library,
                &model.0,
                &mut player,
                &mut transitions,
                DEATH,
                false,
            );
        }
        let sinking = corpse.age - (CORPSE_LINGER_SECS - CORPSE_SINK_SECS);
        if sinking > 0.0 {
            transform.translation.y -= CORPSE_SINK_DEPTH / CORPSE_SINK_SECS * time.delta_secs();
        }
        if corpse.age >= CORPSE_LINGER_SECS {
            commands.entity(entity).despawn();
        }
    }
}

const ROTOR_TURNS_PER_SEC: f32 = 4.0;
const TAIL_ROTOR_SPEEDUP: f32 = 1.6;

/// The spinning parts of an aircraft's model, found once its scene has loaded.
#[derive(Component)]
pub struct Rotors {
    main: (Entity, Quat),
    tail: Option<(Entity, Quat)>,
    angle: f32,
}

pub fn link_rotor_nodes(
    mut commands: Commands,
    aircraft: Query<Entity, (With<Aircraft>, Without<Rotors>)>,
    children: Query<&Children>,
    nodes: Query<(&Name, &Transform)>,
) {
    for entity in &aircraft {
        let find = |wanted: &str| {
            children.iter_descendants(entity).find_map(|node| {
                nodes
                    .get(node)
                    .ok()
                    .filter(|(name, _)| name.as_str() == wanted)
                    .map(|(_, transform)| (node, transform.rotation))
            })
        };
        if let Some(main) = find("Rotor") {
            commands.entity(entity).insert(Rotors {
                main,
                tail: find("TailRotor"),
                angle: 0.0,
            });
        }
    }
}

pub fn spin_rotors(
    time: Res<Time>,
    mut aircraft: Query<&mut Rotors>,
    mut nodes: Query<&mut Transform, Without<Rotors>>,
) {
    for mut rotors in &mut aircraft {
        rotors.angle = (rotors.angle
            + time.delta_secs() * ROTOR_TURNS_PER_SEC * std::f32::consts::TAU)
            % std::f32::consts::TAU;
        let (main, main_rest) = rotors.main;
        if let Ok(mut transform) = nodes.get_mut(main) {
            transform.rotation = main_rest * Quat::from_rotation_y(rotors.angle);
        }
        if let Some((tail, tail_rest)) = rotors.tail
            && let Ok(mut transform) = nodes.get_mut(tail)
        {
            transform.rotation =
                tail_rest * Quat::from_rotation_z(rotors.angle * TAIL_ROTOR_SPEEDUP);
        }
    }
}

/// How a replayed unit moves between two snapshots, which picks its animation.
#[derive(Component)]
pub struct ReplayMotion {
    pub moving: bool,
    pub infantry: bool,
}

pub fn animate_replay_actors(
    library: Res<AnimationLibrary>,
    mut actors: Query<(&AnimatedModel, &mut AnimationLink, &ReplayMotion)>,
    mut players: Query<(&mut AnimationPlayer, &mut AnimationTransitions)>,
) {
    for (model, mut link, motion) in &mut actors {
        let Ok((mut player, mut transitions)) = players.get_mut(link.player) else {
            continue;
        };
        let clip = match (motion.infantry, motion.moving) {
            (true, true) => WALK,
            (true, false) => IDLE,
            (false, _) => DRIVE,
        };
        switch_to(
            &mut link,
            &library,
            &model.0,
            &mut player,
            &mut transitions,
            clip,
            true,
        );
        if !motion.infantry
            && let Some(index) = library.clip(&model.0, DRIVE)
            && let Some(active) = player.animation_mut(index)
        {
            active.set_speed(if motion.moving { 1.0 } else { 0.0 });
        }
    }
}
