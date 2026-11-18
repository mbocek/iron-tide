use bevy::anti_alias::smaa::Smaa;
use bevy::camera::Hdr;
use bevy::input::mouse::{AccumulatedMouseMotion, MouseWheel};
use bevy::light::{CascadeShadowConfigBuilder, DirectionalLightShadowMap};
use bevy::pbr::ScreenSpaceAmbientOcclusion;
use bevy::pbr::{DistanceFog, FogFalloff};
use bevy::picking::hover::HoverMap;
use bevy::picking::pointer::PointerId;
use bevy::post_process::bloom::Bloom;
use bevy::prelude::*;
use bevy::render::view::{ColorGrading, ColorGradingGlobal, ColorGradingSection};
use bevy::window::PrimaryWindow;

use crate::catalog::BuildingKind;
use crate::levels::ActiveLevel;
use crate::map::{MAP_WORLD_SIZE, footprint_center};
use crate::match_flow::Persistent;
use crate::settings::GameSettings;
use crate::terrain::Landscape;
use crate::ui::SIDEBAR_WIDTH;

const PAN_SPEED: f32 = 1.1;
const FAST_PAN_MULTIPLIER: f32 = 2.2;
const EDGE_PAN_MARGIN: f32 = 6.0;
const ROTATE_SPEED: f32 = 1.6;
const ZOOM_STEP: f32 = 5.0;
const ZOOM_SMOOTHING: f32 = 10.0;
const MIN_DISTANCE: f32 = 12.0;
const MAX_DISTANCE: f32 = 95.0;
const START_DISTANCE: f32 = 40.0;
const PITCH: f32 = 0.95;
const VERTICAL_FOV: f32 = std::f32::consts::FRAC_PI_4;
const FOCUS_MARGIN: f32 = 6.0;
const SKY_COLOR: Color = Color::srgb(0.66, 0.74, 0.82);

#[derive(Component)]
pub struct RtsCamera {
    pub focus: Vec3,
    pub distance: f32,
    target_distance: f32,
    yaw: f32,
}

#[derive(Resource, Default)]
pub struct Cursor {
    pub screen: Option<Vec2>,
    pub ground: Option<Vec3>,
    /// True while the pointer is over any interface element, so world clicks must be ignored.
    pub over_interface: bool,
}

fn home_focus(level: &ActiveLevel) -> Vec3 {
    footprint_center(
        level.0.player.site.origin(),
        BuildingKind::CommandCenter.spec().footprint,
    )
}

pub fn focus_on_player_base(level: Res<ActiveLevel>, mut cameras: Query<&mut RtsCamera>) {
    for mut camera in &mut cameras {
        camera.focus = home_focus(&level);
    }
}

pub fn setup_scene(mut commands: Commands) {
    let start_focus = Vec3::new(MAP_WORLD_SIZE * 0.5, 0.0, MAP_WORLD_SIZE * 0.5);
    commands.spawn((
        Persistent,
        Camera3d::default(),
        Hdr,
        Msaa::Off,
        Smaa::default(),
        ScreenSpaceAmbientOcclusion::default(),
        Projection::Perspective(PerspectiveProjection {
            fov: VERTICAL_FOV,
            far: 600.0,
            ..default()
        }),
        Bloom::NATURAL,
        ColorGrading {
            global: ColorGradingGlobal {
                post_saturation: 1.06,
                ..default()
            },
            midtones: ColorGradingSection {
                contrast: 1.08,
                ..default()
            },
            ..default()
        },
        RtsCamera {
            focus: start_focus,
            distance: START_DISTANCE,
            target_distance: START_DISTANCE,
            yaw: 0.0,
        },
        Transform::default(),
        DistanceFog {
            color: SKY_COLOR,
            directional_light_color: Color::srgba(1.0, 0.92, 0.75, 0.35),
            directional_light_exponent: 24.0,
            falloff: FogFalloff::Linear {
                start: 110.0,
                end: 340.0,
            },
        },
    ));

    commands.spawn((
        Persistent,
        DirectionalLight {
            illuminance: 7_000.0,
            shadow_maps_enabled: true,
            color: Color::srgb(1.0, 0.96, 0.9),
            ..default()
        },
        Transform::from_xyz(-48.0, 72.0, 34.0).looking_at(Vec3::ZERO, Vec3::Y),
        CascadeShadowConfigBuilder {
            maximum_distance: 180.0,
            first_cascade_far_bound: 45.0,
            ..default()
        }
        .build(),
    ));
    commands.insert_resource(DirectionalLightShadowMap { size: 4096 });
    commands.insert_resource(GlobalAmbientLight {
        color: Color::srgb(0.72, 0.80, 1.0),
        brightness: 280.0,
        ..default()
    });
    commands.insert_resource(ClearColor(SKY_COLOR));
}

fn keyboard_pan(keys: &ButtonInput<KeyCode>) -> Vec2 {
    let mut pan = Vec2::ZERO;
    if keys.any_pressed([KeyCode::KeyW, KeyCode::ArrowUp]) {
        pan.y -= 1.0;
    }
    if keys.any_pressed([KeyCode::KeyS, KeyCode::ArrowDown]) {
        pan.y += 1.0;
    }
    if keys.any_pressed([KeyCode::KeyA, KeyCode::ArrowLeft]) {
        pan.x -= 1.0;
    }
    if keys.any_pressed([KeyCode::KeyD, KeyCode::ArrowRight]) {
        pan.x += 1.0;
    }
    pan
}

fn edge_pan(window: &Window, cursor: &Cursor) -> Vec2 {
    let Some(screen) = cursor.screen.filter(|_| window.focused) else {
        return Vec2::ZERO;
    };
    let playfield_width = window.width() - SIDEBAR_WIDTH;
    let mut pan = Vec2::ZERO;
    if screen.x < EDGE_PAN_MARGIN {
        pan.x -= 1.0;
    }
    if screen.x > playfield_width - EDGE_PAN_MARGIN && screen.x <= playfield_width {
        pan.x += 1.0;
    }
    if screen.y < EDGE_PAN_MARGIN {
        pan.y -= 1.0;
    }
    if screen.y > window.height() - EDGE_PAN_MARGIN {
        pan.y += 1.0;
    }
    pan
}

pub fn control_camera(
    settings: Res<GameSettings>,
    level: Res<ActiveLevel>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    mouse_motion: Res<AccumulatedMouseMotion>,
    mut wheel: MessageReader<MouseWheel>,
    cursor: Res<Cursor>,
    windows: Query<&Window, With<PrimaryWindow>>,
    time: Res<Time>,
    mut cameras: Query<(&mut RtsCamera, &mut Transform)>,
) {
    let Ok((mut camera, mut transform)) = cameras.single_mut() else {
        return;
    };
    let Ok(window) = windows.single() else {
        return;
    };
    let dt = time.delta_secs();

    if keys.any_just_pressed([KeyCode::Home, KeyCode::Space]) {
        camera.focus = home_focus(&level);
    }
    if keys.just_pressed(KeyCode::KeyR) {
        camera.yaw = 0.0;
    }
    if keys.pressed(KeyCode::KeyQ) {
        camera.yaw += ROTATE_SPEED * dt;
    }
    if keys.pressed(KeyCode::KeyE) {
        camera.yaw -= ROTATE_SPEED * dt;
    }
    for event in wheel.read() {
        if !cursor.over_interface {
            camera.target_distance = (camera.target_distance - event.y.signum() * ZOOM_STEP)
                .clamp(MIN_DISTANCE, MAX_DISTANCE);
        }
    }
    camera.distance += (camera.target_distance - camera.distance) * (ZOOM_SMOOTHING * dt).min(1.0);

    let yaw_rotation = Quat::from_rotation_y(camera.yaw);
    let fast = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let edge = if settings.edge_scroll {
        edge_pan(window, &cursor)
    } else {
        Vec2::ZERO
    };
    let pan = keyboard_pan(&keys) + edge;
    let pan_speed = PAN_SPEED
        * settings.scroll_speed_factor()
        * camera.distance
        * if fast { FAST_PAN_MULTIPLIER } else { 1.0 };
    camera.focus +=
        yaw_rotation * Vec3::new(pan.x, 0.0, pan.y).normalize_or_zero() * pan_speed * dt;

    if mouse.pressed(MouseButton::Middle) {
        let meters_per_pixel = 2.0 * camera.distance * (VERTICAL_FOV * 0.5).tan() / window.height();
        let drag = mouse_motion.delta * meters_per_pixel * Vec2::new(1.0, 1.0 / PITCH.sin());
        camera.focus -= yaw_rotation * Vec3::new(drag.x, 0.0, drag.y);
    }

    camera.focus.x = camera
        .focus
        .x
        .clamp(-FOCUS_MARGIN, MAP_WORLD_SIZE + FOCUS_MARGIN);
    camera.focus.z = camera
        .focus
        .z
        .clamp(-FOCUS_MARGIN, MAP_WORLD_SIZE + FOCUS_MARGIN);
    let ground_under_focus = level
        .0
        .landscape
        .height(camera.focus.x, camera.focus.z)
        .max(0.0);
    camera.focus.y += (ground_under_focus - camera.focus.y) * (dt * 4.0).min(1.0);

    let offset = yaw_rotation * Vec3::new(0.0, PITCH.sin(), PITCH.cos()) * camera.distance;
    *transform =
        Transform::from_translation(camera.focus + offset).looking_at(camera.focus, Vec3::Y);
}

pub fn track_cursor(
    level: Res<ActiveLevel>,
    hover_map: Res<HoverMap>,
    interface_nodes: Query<(), With<Node>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform), With<RtsCamera>>,
    mut cursor: ResMut<Cursor>,
) {
    let (Ok(window), Ok((camera, camera_transform))) = (windows.single(), cameras.single()) else {
        return;
    };
    cursor.screen = window.cursor_position();
    let over_sidebar = cursor
        .screen
        .is_some_and(|screen| screen.x > window.width() - SIDEBAR_WIDTH);
    let over_panel = hover_map
        .get(&PointerId::Mouse)
        .is_some_and(|hits| hits.keys().any(|&entity| interface_nodes.contains(entity)));
    cursor.over_interface = over_sidebar || over_panel;
    cursor.ground = cursor.screen.and_then(|screen| {
        let ray = camera.viewport_to_world(camera_transform, screen).ok()?;
        Some(ray_hit_on_terrain(ray, level.0.landscape))
    });
}

/// Refines a flat-ground hit by re-intersecting at the terrain height under the previous guess.
pub fn ray_hit_on_terrain(ray: Ray3d, landscape: Landscape) -> Vec3 {
    let refinement_steps = 6;
    let mut point = ray.origin;
    let mut ground_height = 0.0;
    for _ in 0..refinement_steps {
        let Some(distance) =
            ray.intersect_plane(Vec3::Y * ground_height, InfinitePlane3d::new(Vec3::Y))
        else {
            break;
        };
        point = ray.get_point(distance);
        ground_height = landscape.height(point.x, point.z);
    }
    point.with_y(ground_height)
}
