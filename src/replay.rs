use std::collections::HashMap;
use std::path::{Path, PathBuf};

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::assets::ModelLibrary;
use crate::catalog::{BuildingKind, Team, UnitKind};
use crate::components::{Building, Health, Mobile, Unit};
use crate::levels::{ActiveLevel, LevelDefinition};
use crate::map::footprint_center;
use crate::match_flow::{GameState, MatchClock};
use crate::movement::FLIGHT_ALTITUDE;
use crate::outcome::GameOutcome;
use crate::settings::game_data_directory;
use crate::ui::{ACCENT, TEXT_MAIN, UiFonts};

const REPLAY_VERSION: u32 = 1;
const REPLAY_DIRECTORY: &str = "replays";
const FRAME_INTERVAL_SECS: f32 = 0.5;
const KEPT_REPLAYS: usize = 10;
/// A match shorter than this is not worth keeping when it is abandoned.
const MIN_ABANDONED_FRAMES: usize = 20;
const SPEEDS: [f32; 4] = [1.0, 2.0, 4.0, 8.0];
const DECIMETRES_PER_METER: f32 = 10.0;

/// Snapshots of every unit and structure twice a second, enough to watch a match again.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Replay {
    pub version: u32,
    pub level: LevelDefinition,
    pub outcome: String,
    pub recorded_secs: f32,
    pub frames: Vec<Frame>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Frame {
    pub time: f32,
    pub actors: Vec<Actor>,
}

/// Packed so a half-hour match stays a few megabytes: replay id, look, team, x and z in decimetres,
/// facing in 1/256 turns and health in percent.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct Actor(u32, u8, u8, i16, i16, u8, u8);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Look {
    Unit(UnitKind),
    Building(BuildingKind),
}

const BUILDING_LOOKS_START: u8 = 100;

fn all_buildings() -> impl Iterator<Item = BuildingKind> {
    std::iter::once(BuildingKind::CommandCenter).chain(BuildingKind::CONSTRUCTIBLE)
}

impl Look {
    fn code(self) -> u8 {
        match self {
            Look::Unit(kind) => UnitKind::ALL.iter().position(|&k| k == kind).unwrap_or(0) as u8,
            Look::Building(kind) => {
                BUILDING_LOOKS_START + all_buildings().position(|k| k == kind).unwrap_or(0) as u8
            }
        }
    }

    fn from_code(code: u8) -> Option<Look> {
        if code < BUILDING_LOOKS_START {
            UnitKind::ALL.get(code as usize).copied().map(Look::Unit)
        } else {
            all_buildings()
                .nth((code - BUILDING_LOOKS_START) as usize)
                .map(Look::Building)
        }
    }
}

impl Actor {
    pub fn new(id: u32, look: Look, team: Team, position: Vec3, facing: f32, health: f32) -> Self {
        let decimetres = |meters: f32| (meters * DECIMETRES_PER_METER).round() as i16;
        let turns =
            (facing.rem_euclid(std::f32::consts::TAU) / std::f32::consts::TAU * 256.0) as u8;
        Actor(
            id,
            look.code(),
            team.index() as u8,
            decimetres(position.x),
            decimetres(position.z),
            turns,
            (health.clamp(0.0, 1.0) * 100.0).round() as u8,
        )
    }

    pub fn id(self) -> u32 {
        self.0
    }

    pub fn look(self) -> Option<Look> {
        Look::from_code(self.1)
    }

    pub fn team(self) -> Team {
        Team::ALL
            .get(self.2 as usize)
            .copied()
            .unwrap_or(Team::Player)
    }

    /// Ground position; the height comes from the landscape when it is shown.
    pub fn position(self) -> Vec2 {
        Vec2::new(self.3 as f32, self.4 as f32) / DECIMETRES_PER_METER
    }

    pub fn facing(self) -> f32 {
        self.5 as f32 / 256.0 * std::f32::consts::TAU
    }
}

/// The frame shown at `time` and how far it is towards the next one, in [0, 1].
pub fn frame_at(frames: &[Frame], time: f32) -> (usize, f32) {
    let next = frames.partition_point(|frame| frame.time <= time);
    let current = next.saturating_sub(1);
    let Some(following) = frames.get(next).filter(|_| next > 0) else {
        return (current, 0.0);
    };
    let span = (following.time - frames[current].time).max(f32::EPSILON);
    (
        current,
        ((time - frames[current].time) / span).clamp(0.0, 1.0),
    )
}

fn blend_facing(from: f32, to: f32, t: f32) -> f32 {
    let turn =
        (to - from + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
    from + turn * t
}

pub fn replay_directory() -> PathBuf {
    game_data_directory().join(REPLAY_DIRECTORY)
}

/// Writes the replay and keeps only the newest few on disk.
pub fn store_replay(directory: &Path, replay: &Replay, stamp: u64) -> Result<PathBuf, String> {
    std::fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    let path = directory.join(format!("replay-{stamp:012}.json"));
    let json = serde_json::to_string(replay).map_err(|error| error.to_string())?;
    std::fs::write(&path, json).map_err(|error| error.to_string())?;
    let mut stored = replay_files(directory);
    while stored.len() > KEPT_REPLAYS {
        let oldest = stored.remove(stored.len() - 1);
        let _ = std::fs::remove_file(oldest);
    }
    Ok(path)
}

/// Replay files, newest first.
fn replay_files(directory: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("replay-") && name.ends_with(".json"))
        })
        .collect();
    files.sort();
    files.reverse();
    files
}

#[derive(Clone, Debug)]
pub struct ReplaySummary {
    pub path: PathBuf,
    pub title: String,
    pub outcome: String,
    pub recorded_secs: f32,
}

#[derive(Deserialize)]
struct ReplayHeader {
    level: LevelHeader,
    outcome: String,
    recorded_secs: f32,
}

#[derive(Deserialize)]
struct LevelHeader {
    name: String,
}

pub fn list_replays(directory: &Path) -> Vec<ReplaySummary> {
    replay_files(directory)
        .into_iter()
        .filter_map(|path| {
            let header: ReplayHeader =
                serde_json::from_str(&std::fs::read_to_string(&path).ok()?).ok()?;
            Some(ReplaySummary {
                path,
                title: header.level.name,
                outcome: header.outcome,
                recorded_secs: header.recorded_secs,
            })
        })
        .collect()
}

pub fn load_replay(path: &Path) -> Result<Replay, String> {
    let json = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
    let replay: Replay = serde_json::from_str(&json).map_err(|error| error.to_string())?;
    if replay.version != REPLAY_VERSION {
        return Err(format!(
            "replay version {} is not supported",
            replay.version
        ));
    }
    Ok(replay)
}

#[derive(Resource, Default)]
pub struct ReplayRecorder {
    frames: Vec<Frame>,
    timer: f32,
    ids: HashMap<Entity, u32>,
    saved: bool,
}

impl ReplayRecorder {
    fn replay_id(&mut self, entity: Entity) -> u32 {
        let next = self.ids.len() as u32;
        *self.ids.entry(entity).or_insert(next)
    }
}

/// Replays show everything; the fog of war belongs to the match.
pub fn clear_the_fog(mut commands: Commands) {
    commands.insert_resource(crate::fog::FogOfWar::new(false));
}

pub fn stop_watching(mut commands: Commands) {
    commands.remove_resource::<ReplayPlayback>();
}

pub fn start_recording(mut commands: Commands) {
    commands.insert_resource(ReplayRecorder::default());
}

pub fn record_replay_frame(
    time: Res<Time>,
    clock: Res<MatchClock>,
    mut recorder: ResMut<ReplayRecorder>,
    units: Query<(Entity, &Unit, &Team, &Transform, &Mobile, &Health)>,
    buildings: Query<(Entity, &Building, &Team, &Transform, &Health)>,
) {
    recorder.timer -= time.delta_secs();
    if recorder.timer > 0.0 || recorder.saved {
        return;
    }
    recorder.timer = FRAME_INTERVAL_SECS;
    let mut actors = Vec::new();
    for (entity, unit, &team, transform, mobile, health) in &units {
        let id = recorder.replay_id(entity);
        let look = Look::Unit(unit.0);
        actors.push(Actor::new(
            id,
            look,
            team,
            transform.translation,
            mobile.facing,
            health.fraction(),
        ));
    }
    for (entity, building, &team, transform, health) in &buildings {
        let id = recorder.replay_id(entity);
        let center = footprint_center(building.origin, building.kind.spec().footprint);
        let facing = transform.rotation.to_euler(EulerRot::YXZ).0;
        let look = Look::Building(building.kind);
        actors.push(Actor::new(
            id,
            look,
            team,
            center,
            facing,
            health.fraction(),
        ));
    }
    recorder.frames.push(Frame {
        time: clock.elapsed_secs,
        actors,
    });
}

fn save_recording(recorder: &mut ReplayRecorder, level: &LevelDefinition, outcome: &str) {
    recorder.saved = true;
    let replay = Replay {
        version: REPLAY_VERSION,
        level: level.clone(),
        outcome: outcome.to_string(),
        recorded_secs: recorder.frames.last().map_or(0.0, |frame| frame.time),
        frames: std::mem::take(&mut recorder.frames),
    };
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    if let Err(error) = store_replay(&replay_directory(), &replay, stamp) {
        warn!("could not store the replay: {error}");
    }
}

pub fn save_replay_when_decided(
    outcome: Res<GameOutcome>,
    level: Res<ActiveLevel>,
    mut recorder: ResMut<ReplayRecorder>,
) {
    if recorder.saved || *outcome == GameOutcome::Ongoing {
        return;
    }
    let result = if *outcome == GameOutcome::Victory {
        "Victory"
    } else {
        "Defeat"
    };
    save_recording(&mut recorder, &level.0, result);
}

/// Closing the game in the middle of a match keeps what was recorded so far.
pub fn save_replay_on_exit(
    mut exits: MessageReader<AppExit>,
    state: Res<State<GameState>>,
    level: Res<ActiveLevel>,
    recorder: Option<ResMut<ReplayRecorder>>,
) {
    let exiting = exits.read().next().is_some();
    if exiting && *state.get() == GameState::Playing {
        save_abandoned_replay(level, recorder);
    }
}

pub fn save_abandoned_replay(level: Res<ActiveLevel>, recorder: Option<ResMut<ReplayRecorder>>) {
    let Some(mut recorder) = recorder else { return };
    if !recorder.saved && recorder.frames.len() >= MIN_ABANDONED_FRAMES {
        save_recording(&mut recorder, &level.0, "Unfinished");
    }
}

/// The replay being watched and where in it the viewer is.
#[derive(Resource)]
pub struct ReplayPlayback {
    replay: Replay,
    clock: f32,
    speed: usize,
    paused: bool,
    actors: HashMap<u32, Entity>,
}

impl ReplayPlayback {
    pub fn new(replay: Replay) -> Self {
        Self {
            replay,
            clock: 0.0,
            speed: 0,
            paused: false,
            actors: HashMap::new(),
        }
    }
}

#[derive(Component)]
pub struct ReplayActor;

#[derive(Component)]
pub struct ReplayHud;

pub fn spawn_replay_hud(mut commands: Commands, fonts: Res<UiFonts>) {
    commands
        .spawn((
            DespawnOnExit(GameState::Replay),
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(14.0),
                left: Val::Px(0.0),
                right: Val::Px(0.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: Val::Px(2.0),
                ..default()
            },
            Pickable::IGNORE,
        ))
        .with_children(|hud| {
            hud.spawn((
                ReplayHud,
                fonts.heading("", 20.0, ACCENT),
                TextShadow::default(),
            ));
            hud.spawn((
                fonts.text(
                    "Space pause  ·  + / - speed  ·  Esc back to the menu",
                    15.0,
                    TEXT_MAIN,
                ),
                TextShadow::default(),
            ));
        });
}

pub fn control_replay(
    keys: Res<ButtonInput<KeyCode>>,
    mut playback: ResMut<ReplayPlayback>,
    mut next_state: ResMut<NextState<GameState>>,
) {
    if keys.just_pressed(KeyCode::Space) {
        playback.paused = !playback.paused;
    }
    if keys.any_just_pressed([KeyCode::Equal, KeyCode::NumpadAdd]) {
        playback.speed = (playback.speed + 1).min(SPEEDS.len() - 1);
    }
    if keys.any_just_pressed([KeyCode::Minus, KeyCode::NumpadSubtract]) {
        playback.speed = playback.speed.saturating_sub(1);
    }
    if keys.just_pressed(KeyCode::Escape) {
        next_state.set(GameState::MainMenu);
    }
}

fn spawn_actor(commands: &mut Commands, models: &ModelLibrary, actor: Actor) -> Option<Entity> {
    let model = match actor.look()? {
        Look::Unit(kind) => models.unit(kind, actor.team()),
        Look::Building(kind) => models.building(kind, actor.team()),
    };
    Some(
        commands
            .spawn((
                ReplayActor,
                DespawnOnExit(GameState::Replay),
                WorldAssetRoot(model),
                Transform::default(),
            ))
            .id(),
    )
}

pub fn play_replay(
    mut commands: Commands,
    time: Res<Time<Real>>,
    level: Res<ActiveLevel>,
    models: Res<ModelLibrary>,
    mut playback: ResMut<ReplayPlayback>,
    mut transforms: Query<&mut Transform, With<ReplayActor>>,
) {
    let playback = &mut *playback;
    let end = playback.replay.recorded_secs;
    if !playback.paused {
        playback.clock = (playback.clock + time.delta_secs() * SPEEDS[playback.speed]).min(end);
    }
    let frames = &playback.replay.frames;
    if frames.is_empty() {
        return;
    }
    let (index, blend) = frame_at(frames, playback.clock);
    let current = &frames[index];
    let following: HashMap<u32, Actor> = frames
        .get(index + 1)
        .map(|frame| {
            frame
                .actors
                .iter()
                .map(|actor| (actor.id(), *actor))
                .collect()
        })
        .unwrap_or_default();

    let present: Vec<u32> = current.actors.iter().map(|actor| actor.id()).collect();
    playback.actors.retain(|id, entity| {
        let still_here = present.contains(id);
        if !still_here {
            commands.entity(*entity).despawn();
        }
        still_here
    });
    for actor in &current.actors {
        let next = following.get(&actor.id()).copied().unwrap_or(*actor);
        let ground = actor.position().lerp(next.position(), blend);
        let facing = blend_facing(actor.facing(), next.facing(), blend);
        let altitude = match actor.look() {
            Some(Look::Unit(kind)) if kind.flies() => FLIGHT_ALTITUDE,
            _ => 0.0,
        };
        let translation = level
            .0
            .landscape
            .on_ground(Vec3::new(ground.x, 0.0, ground.y))
            + Vec3::Y * altitude;
        let placed =
            Transform::from_translation(translation).with_rotation(Quat::from_rotation_y(facing));
        match playback.actors.get(&actor.id()) {
            Some(&entity) => {
                if let Ok(mut transform) = transforms.get_mut(entity) {
                    *transform = placed;
                }
            }
            None => {
                if let Some(entity) = spawn_actor(&mut commands, &models, *actor) {
                    commands.entity(entity).insert(placed);
                    playback.actors.insert(actor.id(), entity);
                }
            }
        }
    }
}

pub fn refresh_replay_hud(
    level: Res<ActiveLevel>,
    playback: Res<ReplayPlayback>,
    mut huds: Query<&mut Text, With<ReplayHud>>,
) {
    let clock = |secs: f32| format!("{:02}:{:02}", secs as u32 / 60, secs as u32 % 60);
    let state = if playback.paused {
        "paused".to_string()
    } else {
        format!("{}x", SPEEDS[playback.speed])
    };
    let line = format!(
        "REPLAY  {}  ·  {} / {}  ·  {}  ·  {}",
        level.0.name,
        clock(playback.clock),
        clock(playback.replay.recorded_secs),
        state,
        playback.replay.outcome
    );
    for mut text in &mut huds {
        if text.0 != line {
            text.0 = line.clone();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(time: f32, x: f32) -> Frame {
        let actor = Actor::new(
            7,
            Look::Unit(UnitKind::Tank),
            Team::Amber,
            Vec3::new(x, 0.0, 12.0),
            1.0,
            0.5,
        );
        Frame {
            time,
            actors: vec![actor],
        }
    }

    #[test]
    fn actors_pack_and_unpack_what_matters() {
        let actor = Actor::new(
            42,
            Look::Building(BuildingKind::Wall),
            Team::Violet,
            Vec3::new(31.25, 5.0, 77.0),
            -1.0,
            0.333,
        );
        assert_eq!(actor.id(), 42);
        assert_eq!(actor.look(), Some(Look::Building(BuildingKind::Wall)));
        assert_eq!(actor.team(), Team::Violet);
        assert!((actor.position() - Vec2::new(31.3, 77.0)).length() < 0.05);
        assert!((actor.facing() - (std::f32::consts::TAU - 1.0)).abs() < 0.03);
        assert_eq!(actor.6, 33);
    }

    #[test]
    fn playback_finds_the_frame_and_the_blend_towards_the_next() {
        let frames = vec![frame(0.0, 10.0), frame(0.5, 20.0), frame(1.0, 30.0)];
        assert_eq!(frame_at(&frames, 0.0), (0, 0.0));
        let (index, blend) = frame_at(&frames, 0.75);
        assert_eq!(index, 1);
        assert!((blend - 0.5).abs() < 1e-5);
        assert_eq!(frame_at(&frames, 9.0), (2, 0.0));
    }

    #[test]
    fn only_the_newest_replays_are_kept() {
        let directory =
            std::env::temp_dir().join(format!("iron_tide_replays_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        let replay = Replay {
            version: REPLAY_VERSION,
            level: LevelDefinition::default(),
            outcome: "Victory".into(),
            recorded_secs: 1.0,
            frames: vec![frame(0.0, 1.0), frame(1.0, 2.0)],
        };
        for stamp in 0..(KEPT_REPLAYS as u64 + 3) {
            store_replay(&directory, &replay, stamp).unwrap();
        }
        let listed = list_replays(&directory);
        assert_eq!(listed.len(), KEPT_REPLAYS);
        assert!(
            listed[0]
                .path
                .ends_with(format!("replay-{:012}.json", KEPT_REPLAYS + 2))
        );
        assert_eq!(load_replay(&listed[0].path).unwrap().frames, replay.frames);
        let _ = std::fs::remove_dir_all(&directory);
    }
}
