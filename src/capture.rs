use crate::lobby::{NetLobby, network_level};
use crate::main_menu::MainMenu;
use crate::replay::{ReplayPlayback, list_replays, load_replay, replay_directory};
use bevy::camera::RenderTarget;
use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use bevy::input::InputSystems;
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
use bevy::window::{CursorOptions, PrimaryWindow};

use crate::camera::RtsCamera;
use crate::catalog::Team;
use crate::components::{Selected, Unit};
use crate::levels::{
    ActiveLevel, SkirmishOptions, campaign, custom_levels, custom_levels_directory,
};
use crate::map::cell_center;
use crate::match_flow::GameState;
use crate::menu::MenuState;
use crate::savegame::SaveRequests;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};

const CAPTURE_PATH_VAR: &str = "IRON_TIDE_CAPTURE";
const CAPTURE_DELAY_VAR: &str = "IRON_TIDE_CAPTURE_AFTER_SECS";
const CAPTURE_FOCUS_VAR: &str = "IRON_TIDE_CAPTURE_FOCUS_CELL";
const CAPTURE_SAVE_SLOT_VAR: &str = "IRON_TIDE_CAPTURE_SAVE_SLOT";
const CAPTURE_LOAD_SLOT_VAR: &str = "IRON_TIDE_CAPTURE_LOAD_SLOT";
const CAPTURE_MISSION_VAR: &str = "IRON_TIDE_CAPTURE_MISSION";
const CAPTURE_REPLAY_VAR: &str = "IRON_TIDE_CAPTURE_REPLAY";
const CAPTURE_SELECT_ALL_VAR: &str = "IRON_TIDE_CAPTURE_SELECT_ALL";
const CAPTURE_ZOOM_VAR: &str = "IRON_TIDE_CAPTURE_ZOOM";
const CAPTURE_MENU_VAR: &str = "IRON_TIDE_CAPTURE_MENU";
/// `IRON_TIDE_CAPTURE_HOST=1` hosts a network battle and `IRON_TIDE_CAPTURE_JOIN=127.0.0.1` joins one.
const CAPTURE_HOST_VAR: &str = "IRON_TIDE_CAPTURE_HOST";
const CAPTURE_JOIN_VAR: &str = "IRON_TIDE_CAPTURE_JOIN";
const LOAD_AFTER_SECS: f32 = 1.5;
const DEFAULT_CAPTURE_DELAY_SECS: f32 = 5.0;
const FRAMES_TO_FLUSH_SCREENSHOT: u32 = 10;
const CAPTURE_SIZE: UVec2 = UVec2::new(1600, 900);

pub fn is_capture_run() -> bool {
    std::env::var_os(CAPTURE_PATH_VAR).is_some()
}

/// Development aid: with `IRON_TIDE_CAPTURE=shot.png` the game saves one screenshot and quits.
/// `IRON_TIDE_CAPTURE_SAVE_SLOT` / `_LOAD_SLOT` exercise saving and loading in the same unattended run.
/// Input is ignored meanwhile so a stray mouse on the desktop cannot steer the run.
pub struct CapturePlugin;

#[derive(Resource)]
struct PendingCapture {
    path: String,
    save_slot: Option<usize>,
    load_slot: Option<usize>,
    remaining_secs: f32,
    frames_after_shot: Option<u32>,
}

impl Plugin for CapturePlugin {
    fn build(&self, app: &mut App) {
        let Ok(path) = std::env::var(CAPTURE_PATH_VAR) else {
            return;
        };
        let delay = std::env::var(CAPTURE_DELAY_VAR)
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(DEFAULT_CAPTURE_DELAY_SECS);
        let slot_from = |var: &str| std::env::var(var).ok().and_then(|value| value.parse().ok());
        app.insert_resource(PendingCapture {
            path,
            save_slot: slot_from(CAPTURE_SAVE_SLOT_VAR),
            load_slot: slot_from(CAPTURE_LOAD_SLOT_VAR),
            remaining_secs: delay,
            frames_after_shot: None,
        })
        .add_systems(Startup, let_mouse_pass_through)
        .add_systems(
            PostStartup,
            (
                spawn_capture_camera,
                start_requested_mission.after(crate::match_flow::finish_booting),
            ),
        )
        .add_systems(PreUpdate, discard_keyboard.after(InputSystems))
        .add_systems(Update, (follow_main_camera, capture_then_exit).chain())
        .add_systems(Update, accept_network_offer)
        .add_systems(
            Update,
            (select_all_own_units, zoom_camera, open_requested_menu_page)
                .run_if(in_state(GameState::Playing)),
        );
        if let Some(cell) = std::env::var(CAPTURE_FOCUS_VAR)
            .ok()
            .and_then(|value| parse_cell(&value))
        {
            app.insert_resource(CaptureFocus(cell)).add_systems(
                OnEnter(GameState::Playing),
                focus_camera.after(crate::camera::focus_on_player_base),
            );
        }
    }
}

/// Screenshots of the window surface come out black on Bevy 0.20 here, so the capture renders
/// through a copy of the game camera into an offscreen image instead, interface included.
#[derive(Resource)]
struct CaptureTarget(Handle<Image>);

#[derive(Component)]
struct CaptureCamera;

fn spawn_capture_camera(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut game_cameras: Query<(Entity, &mut Camera), With<RtsCamera>>,
) {
    let Ok((game_camera, mut window_view)) = game_cameras.single_mut() else {
        return;
    };
    let target = images.add(Image::new_target_texture(
        CAPTURE_SIZE.x,
        CAPTURE_SIZE.y,
        TextureFormat::Rgba8UnormSrgb,
        None,
    ));
    commands
        .entity(game_camera)
        .clone_and_spawn()
        .remove::<RtsCamera>()
        .insert((
            CaptureCamera,
            Camera {
                is_active: true,
                ..window_view.clone()
            },
            RenderTarget::from(target.clone()),
            IsDefaultUiCamera,
        ));
    commands.insert_resource(CaptureTarget(target));
    // Rendering the window too would double the frame cost and skew the fps a capture run reports.
    window_view.is_active = false;
}

fn follow_main_camera(
    game_cameras: Query<&Transform, (With<RtsCamera>, Without<CaptureCamera>)>,
    mut capture_cameras: Query<&mut Transform, With<CaptureCamera>>,
) {
    let Ok(game_view) = game_cameras.single() else {
        return;
    };
    for mut view in &mut capture_cameras {
        *view = *game_view;
    }
}

fn capture_then_exit(
    mut commands: Commands,
    time: Res<Time<Real>>,
    target: Option<Res<CaptureTarget>>,
    diagnostics: Res<DiagnosticsStore>,
    mut requests: ResMut<SaveRequests>,
    mut capture: ResMut<PendingCapture>,
    mut exit: MessageWriter<AppExit>,
) {
    if let Some(frames) = capture.frames_after_shot.as_mut() {
        *frames += 1;
        if *frames > FRAMES_TO_FLUSH_SCREENSHOT {
            exit.write(AppExit::Success);
        }
        return;
    }
    capture.remaining_secs -= time.delta_secs();
    if time.elapsed_secs() > LOAD_AFTER_SECS
        && let Some(slot) = capture.load_slot.take()
    {
        requests.load = Some(slot);
    }
    let Some(target) = target else { return };
    if capture.remaining_secs <= 0.0 {
        let fps = diagnostics
            .get(&FrameTimeDiagnosticsPlugin::FPS)
            .and_then(|fps| fps.smoothed())
            .unwrap_or_default();
        let entities = diagnostics
            .get(&bevy::diagnostic::EntityCountDiagnosticsPlugin::ENTITY_COUNT)
            .and_then(|count| count.value())
            .unwrap_or_default();
        info!("capture: average {fps:.1} fps, {entities:.0} entities");
        requests.save = capture.save_slot.take();
        commands
            .spawn(Screenshot::image(target.0.clone()))
            .observe(save_to_disk(capture.path.clone()));
        capture.frames_after_shot = Some(0);
    }
}

#[derive(Resource)]
struct CaptureFocus(IVec2);

fn parse_cell(value: &str) -> Option<IVec2> {
    let (x, y) = value.split_once(',')?;
    Some(IVec2::new(x.trim().parse().ok()?, y.trim().parse().ok()?))
}

fn focus_camera(focus: Res<CaptureFocus>, mut cameras: Query<&mut RtsCamera>) {
    for mut camera in &mut cameras {
        camera.focus = cell_center(focus.0);
    }
}

/// `IRON_TIDE_CAPTURE_SELECT_ALL=1` selects every own unit, to show the selection marks.
fn select_all_own_units(
    mut commands: Commands,
    units: Query<(Entity, &Team), (With<Unit>, Without<Selected>)>,
) {
    if std::env::var_os(CAPTURE_SELECT_ALL_VAR).is_none() {
        return;
    }
    for (entity, team) in &units {
        if *team == Team::Player {
            commands.entity(entity).insert(Selected);
        }
    }
}

/// `IRON_TIDE_CAPTURE_ZOOM=150` sets the camera distance in meters.
fn zoom_camera(mut cameras: Query<&mut RtsCamera>) {
    let Some(distance) = std::env::var(CAPTURE_ZOOM_VAR)
        .ok()
        .and_then(|value| value.parse().ok())
    else {
        return;
    };
    for mut camera in &mut cameras {
        camera.distance = distance;
        camera.target_distance = distance;
    }
}

/// `IRON_TIDE_CAPTURE_MENU=keys` opens the pause menu on that page (game, graphics, keys, ...).
fn open_requested_menu_page(mut menu: ResMut<MenuState>) {
    if let Ok(page) = std::env::var(CAPTURE_MENU_VAR) {
        menu.show_page_named(&page);
    }
}

/// A capture run that hosts or joins shows the network page, and a joining one accepts the offer at once.
fn accept_network_offer(mut lobby: ResMut<NetLobby>, menu: Option<ResMut<MainMenu>>) {
    let joining = std::env::var_os(CAPTURE_JOIN_VAR).is_some();
    if !joining && std::env::var_os(CAPTURE_HOST_VAR).is_none() {
        return;
    }
    if let Some(mut menu) = menu {
        menu.show_network_page();
    }
    if joining && lobby.can_accept_offer() {
        lobby.accept_offer();
    }
}

fn let_mouse_pass_through(mut cursors: Query<&mut CursorOptions, With<PrimaryWindow>>) {
    for mut cursor in &mut cursors {
        cursor.hit_test = false;
    }
}

fn discard_keyboard(mut keys: ResMut<ButtonInput<KeyCode>>) {
    keys.reset_all();
}

/// `IRON_TIDE_CAPTURE_MISSION=2` starts the second campaign mission instead of waiting in the main menu;
/// numbers past the campaign continue with the custom levels.
/// `IRON_TIDE_CAPTURE_REPLAY=1` watches the newest replay instead of playing.
fn start_requested_mission(
    mut commands: Commands,
    mut next_state: ResMut<NextState<GameState>>,
    mut lobby: ResMut<NetLobby>,
) {
    if std::env::var_os(CAPTURE_HOST_VAR).is_some() {
        lobby.host(network_level(&SkirmishOptions::default()));
        return;
    }
    if let Ok(address) = std::env::var(CAPTURE_JOIN_VAR) {
        lobby.join(&address);
        return;
    }
    let newest_replay = std::env::var_os(CAPTURE_REPLAY_VAR)
        .and_then(|_| list_replays(&replay_directory()).into_iter().next())
        .and_then(|summary| load_replay(&summary.path).ok());
    if let Some(replay) = newest_replay {
        commands.insert_resource(ActiveLevel(replay.level.clone()));
        commands.insert_resource(ReplayPlayback::new(replay));
        next_state.set(GameState::Replay);
        return;
    }
    let Some(number) = std::env::var(CAPTURE_MISSION_VAR)
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
    else {
        next_state.set(GameState::Playing);
        return;
    };
    let campaign_then_custom = campaign()
        .into_iter()
        .chain(custom_levels(&custom_levels_directory()));
    if let Some(mission) = campaign_then_custom
        .into_iter()
        .nth(number.saturating_sub(1))
    {
        commands.insert_resource(ActiveLevel(mission.level));
    }
    next_state.set(GameState::Playing);
}
