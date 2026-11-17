use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::ai::AiCommanders;
use crate::assets::ModelLibrary;
use crate::camera::RtsCamera;
use crate::catalog::{BuildingKind, Team, UnitKind};
use crate::components::*;
use crate::economy::Economies;
use crate::fog::FogOfWar;
use crate::intel::Intel;
use crate::levels::{ActiveLevel, LevelDefinition};
use crate::map::{GameMap, cell_of_index, spawn_crystal_visual};
use crate::match_flow::{GameState, MatchClock};
use crate::notifications::Notifications;
use crate::production::{BuildItem, Lane, Production};
use crate::settings::game_data_directory;
use crate::spawn::{spawn_building, spawn_unit};
use crate::structures::{BeingSold, Repairing};
use crate::veterancy::Veterancy;

/// Bumped whenever the save layout changes incompatibly; older files are refused rather than misread.
const SAVE_VERSION: u32 = 2;
const SAVES_DIRECTORY: &str = "saves";
pub const QUICK_SAVE_SLOT: usize = 0;
pub const NAMED_SLOTS: std::ops::RangeInclusive<usize> = 1..=3;

#[derive(Serialize, Deserialize, Clone)]
struct SavedBuilding {
    team: Team,
    kind: BuildingKind,
    origin: [i32; 2],
    health: f32,
    #[serde(default)]
    rally: Option<[i32; 2]>,
    #[serde(default)]
    repairing: bool,
}

#[derive(Serialize, Deserialize, Clone)]
struct SavedUnit {
    team: Team,
    kind: UnitKind,
    position: [f32; 3],
    facing: f32,
    health: f32,
    cargo: u32,
    #[serde(default)]
    orders: Vec<Order>,
    #[serde(default)]
    experience: f32,
}

#[derive(Serialize, Deserialize, Clone)]
struct SavedLane {
    team: Team,
    lane: Lane,
    queue: Vec<BuildItem>,
    progress_secs: f32,
    ready: Option<BuildingKind>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct SaveGame {
    version: u32,
    saved_at_unix_secs: u64,
    level: LevelDefinition,
    match_secs: f32,
    credits: Vec<i32>,
    crystal: Vec<u32>,
    buildings: Vec<SavedBuilding>,
    units: Vec<SavedUnit>,
    production: Vec<SavedLane>,
    ai: AiCommanders,
    camera_focus: [f32; 3],
    #[serde(default)]
    explored: Vec<bool>,
    #[serde(default)]
    explored_by_syndicate: Vec<bool>,
}

/// A save waiting to be restored once the next match's map has been generated from its level.
#[derive(Resource, Clone)]
pub struct PendingLoad(pub SaveGame);

/// Where save files are written; tests point it at a scratch directory.
#[derive(Resource, Clone)]
pub struct SaveDirectory(pub PathBuf);

impl Default for SaveDirectory {
    fn default() -> Self {
        Self(game_data_directory().join(SAVES_DIRECTORY))
    }
}

#[derive(Resource, Default)]
pub struct SaveRequests {
    pub save: Option<usize>,
    pub load: Option<usize>,
}

fn slot_path(directory: &SaveDirectory, slot: usize) -> PathBuf {
    let file = if slot == QUICK_SAVE_SLOT {
        "quicksave.json".to_string()
    } else {
        format!("slot{slot}.json")
    };
    directory.0.join(file)
}

pub fn slot_label(slot: usize) -> String {
    if slot == QUICK_SAVE_SLOT {
        "Quick save".to_string()
    } else {
        format!("Slot {slot}")
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

fn describe_age(saved_at: u64) -> String {
    let minutes = unix_now().saturating_sub(saved_at) / 60;
    match minutes {
        0 => "just now".to_string(),
        1..=59 => format!("{minutes} min ago"),
        60..=1439 => format!("{} h ago", minutes / 60),
        _ => format!("{} days ago", minutes / 1440),
    }
}

/// One-line description of what a slot holds, or `None` when it is empty or unreadable.
pub fn slot_summary(directory: &SaveDirectory, slot: usize) -> Option<String> {
    let json = std::fs::read_to_string(slot_path(directory, slot)).ok()?;
    let save: SaveGame = serde_json::from_str(&json).ok()?;
    let own_units = save
        .units
        .iter()
        .filter(|unit| unit.team == Team::Player)
        .count();
    Some(format!(
        "{}  -  ${}, {} units",
        describe_age(save.saved_at_unix_secs),
        save.credits[Team::Player.index()],
        own_units
    ))
}

pub fn quick_save_keys(keys: Res<ButtonInput<KeyCode>>, mut requests: ResMut<SaveRequests>) {
    if keys.just_pressed(KeyCode::F5) {
        requests.save = Some(QUICK_SAVE_SLOT);
    }
    if keys.just_pressed(KeyCode::F9) {
        requests.load = Some(QUICK_SAVE_SLOT);
    }
}

pub fn write_requested_save(
    directory: Res<SaveDirectory>,
    mut requests: ResMut<SaveRequests>,
    mut notifications: ResMut<Notifications>,
    level: Res<ActiveLevel>,
    clock: Res<MatchClock>,
    map: Res<GameMap>,
    economies: Res<Economies>,
    production: Res<Production>,
    commanders: Res<AiCommanders>,
    fog: Res<FogOfWar>,
    intel: Res<Intel>,
    cameras: Query<&RtsCamera>,
    buildings: Query<
        (
            &Building,
            &Team,
            &Health,
            Option<&RallyPoint>,
            Has<Repairing>,
        ),
        Without<BeingSold>,
    >,
    units: Query<(
        &Unit,
        &Team,
        &Transform,
        &Mobile,
        &Health,
        Option<&Harvester>,
        Option<&Orders>,
        Option<&Veterancy>,
    )>,
) {
    let Some(slot) = requests.save.take() else {
        return;
    };
    let save = SaveGame {
        version: SAVE_VERSION,
        saved_at_unix_secs: unix_now(),
        level: level.0.clone(),
        match_secs: clock.elapsed_secs,
        credits: Team::ALL
            .iter()
            .map(|&team| economies.of(team).credits)
            .collect(),
        crystal: map.crystal_amounts(),
        buildings: buildings
            .iter()
            .map(
                |(building, &team, health, rally, repairing)| SavedBuilding {
                    team,
                    kind: building.kind,
                    origin: building.origin.to_array(),
                    health: health.current,
                    rally: rally.map(|rally| rally.0.to_array()),
                    repairing,
                },
            )
            .collect(),
        units: units
            .iter()
            .map(
                |(unit, &team, transform, mobile, health, harvester, orders, veterancy)| {
                    SavedUnit {
                        team,
                        kind: unit.0,
                        position: transform.translation.to_array(),
                        facing: mobile.facing,
                        health: health.current,
                        cargo: harvester.map_or(0, |h| h.cargo),
                        orders: orders.map_or_else(Vec::new, |orders| {
                            orders
                                .current
                                .iter()
                                .chain(orders.queued.iter())
                                .copied()
                                .collect()
                        }),
                        experience: veterancy.map_or(0.0, |veterancy| veterancy.experience),
                    }
                },
            )
            .collect(),
        production: Team::ALL
            .into_iter()
            .flat_map(|team| Lane::ALL.map(|lane| (team, lane)))
            .map(|(team, lane)| {
                let state = production.lane(team, lane);
                SavedLane {
                    team,
                    lane,
                    queue: state.queue.iter().copied().collect(),
                    progress_secs: state.progress_secs,
                    ready: state.ready,
                }
            })
            .collect(),
        ai: commanders.clone(),
        camera_focus: cameras
            .single()
            .map_or([0.0; 3], |camera| camera.focus.to_array()),
        explored: fog.explored_cells(),
        explored_by_syndicate: intel.syndicate_vision.explored_cells(),
    };
    let path = slot_path(&directory, slot);
    let written = path
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .map_err(|error| error.to_string())
        .and_then(|_| serde_json::to_string(&save).map_err(|error| error.to_string()))
        .and_then(|json| std::fs::write(&path, json).map_err(|error| error.to_string()));
    match written {
        Ok(()) => notifications.push(format!("Game saved - {}", slot_label(slot))),
        Err(error) => {
            warn!("could not write {}: {error}", path.display());
            notifications.push("Saving failed");
        }
    }
}

fn read_save(directory: &SaveDirectory, slot: usize) -> Result<SaveGame, &'static str> {
    let json = std::fs::read_to_string(slot_path(directory, slot))
        .map_err(|_| "No saved game in that slot")?;
    #[derive(Deserialize)]
    struct VersionOnly {
        version: u32,
    }
    let version: VersionOnly = serde_json::from_str(&json).map_err(|_| "Saved game is damaged")?;
    if version.version != SAVE_VERSION {
        return Err("Saved game is from an incompatible version");
    }
    serde_json::from_str(&json).map_err(|_| "Saved game is damaged")
}

/// Starts a fresh match on the saved level; `restore_pending_load` fills it in once the map exists.
pub fn begin_requested_load(
    mut commands: Commands,
    directory: Res<SaveDirectory>,
    mut requests: ResMut<SaveRequests>,
    notifications: Option<ResMut<Notifications>>,
    mut next_state: ResMut<NextState<GameState>>,
) {
    let Some(slot) = requests.load.take() else {
        return;
    };
    match read_save(&directory, slot) {
        Ok(save) => {
            commands.insert_resource(ActiveLevel(save.level.clone()));
            commands.insert_resource(PendingLoad(save));
            next_state.set(GameState::Restarting);
        }
        Err(reason) => {
            if let Some(mut notifications) = notifications {
                notifications.push(reason);
            }
            warn!("loading slot {slot} failed: {reason}");
        }
    }
}

pub fn restore_pending_load(
    mut commands: Commands,
    pending: Option<Res<PendingLoad>>,
    models: Res<ModelLibrary>,
    mut map: ResMut<GameMap>,
    mut economies: ResMut<Economies>,
    mut production: ResMut<Production>,
    mut commanders: ResMut<AiCommanders>,
    mut clock: ResMut<MatchClock>,
    mut notifications: ResMut<Notifications>,
    mut fog: ResMut<FogOfWar>,
    mut intel: ResMut<Intel>,
    mut cameras: Query<&mut RtsCamera>,
) {
    let Some(pending) = pending else { return };
    let save = pending.0.clone();
    commands.remove_resource::<PendingLoad>();

    map.reset_for_load(save.crystal);
    let mut rng = rand::rng();
    for (index, amount) in map.crystal_amounts().into_iter().enumerate() {
        if amount > 0 {
            spawn_crystal_visual(
                &mut commands,
                &models,
                &mut rng,
                cell_of_index(index),
                amount,
            );
        }
    }
    for saved in &save.buildings {
        let origin = IVec2::from_array(saved.origin);
        let entity = spawn_building(
            &mut commands,
            &models,
            &mut map,
            saved.kind,
            saved.team,
            origin,
        );
        commands.entity(entity).insert(Health {
            current: saved.health,
            max: saved.kind.spec().max_health,
        });
        if let Some(rally) = saved.rally {
            commands
                .entity(entity)
                .insert(RallyPoint(IVec2::from_array(rally)));
        }
        if saved.repairing {
            commands.entity(entity).insert(Repairing::default());
        }
    }
    for saved in &save.units {
        let position = Vec3::from_array(saved.position);
        let entity = spawn_unit(
            &mut commands,
            &models,
            saved.kind,
            saved.team,
            position,
            saved.facing,
        );
        let (veterancy, health) = Veterancy::restored(saved.kind, saved.experience, saved.health);
        commands.entity(entity).insert(health);
        if saved.experience > 0.0 {
            commands.entity(entity).insert(veterancy);
        }
        let mut planned = saved.orders.iter().copied();
        commands.entity(entity).insert(Orders {
            current: planned.next(),
            queued: planned.collect(),
        });
        if saved.kind == UnitKind::Harvester {
            commands.entity(entity).insert(Harvester {
                state: HarvestState::SeekingField,
                cargo: saved.cargo,
                preferred_field: None,
            });
        }
    }
    for (team, credits) in Team::ALL.into_iter().zip(&save.credits) {
        economies.of_mut(team).credits = *credits;
    }
    for (team, enemy) in save.level.opponents() {
        economies.of_mut(team).income_rate = enemy.difficulty.income_rate();
    }
    for saved in save.production {
        let lane = production.lane_mut(saved.team, saved.lane);
        lane.queue = saved.queue.into();
        lane.progress_secs = saved.progress_secs;
        lane.ready = saved.ready;
    }
    *commanders = save.ai;
    clock.elapsed_secs = save.match_secs;
    fog.restore_explored(&save.explored);
    intel
        .syndicate_vision
        .restore_explored(&save.explored_by_syndicate);
    for mut camera in &mut cameras {
        camera.focus = Vec3::from_array(save.camera_focus);
    }
    notifications.push("Game loaded");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slot_files_are_distinct() {
        let directory = SaveDirectory(PathBuf::from("/tmp/saves"));
        let paths: Vec<PathBuf> = std::iter::once(QUICK_SAVE_SLOT)
            .chain(NAMED_SLOTS)
            .map(|slot| slot_path(&directory, slot))
            .collect();
        let mut unique = paths.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), paths.len());
    }

    #[test]
    fn save_age_reads_naturally() {
        let now = unix_now();
        assert_eq!(describe_age(now), "just now");
        assert_eq!(describe_age(now - 5 * 60), "5 min ago");
        assert_eq!(describe_age(now - 3 * 3600), "3 h ago");
        assert_eq!(describe_age(now - 2 * 86_400), "2 days ago");
    }

    #[test]
    fn saves_from_other_versions_are_refused() {
        let directory = SaveDirectory(
            std::env::temp_dir().join(format!("iron_tide_version_test_{}", std::process::id())),
        );
        std::fs::create_dir_all(&directory.0).unwrap();
        std::fs::write(slot_path(&directory, 1), r#"{"version": 999}"#).unwrap();
        assert!(read_save(&directory, 1).is_err());
        assert!(
            read_save(&directory, 2).is_err(),
            "an empty slot is not loadable"
        );
        std::fs::remove_dir_all(&directory.0).unwrap();
    }
}
