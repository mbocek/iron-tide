use bevy::asset::RenderAssetUsages;
use bevy::image::ImageSampler;
use bevy::picking::backend::HitData;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::window::PrimaryWindow;

use crate::camera::{RtsCamera, ray_hit_on_terrain};
use crate::catalog::Team;
use crate::components::{Building, Unit};
use crate::map::{CELL_SIZE, GameMap, MAP_CELLS, MAP_WORLD_SIZE, footprint_cells};
use crate::terrain::{GroundLayers, river_depth};
use crate::ui::SIDEBAR_WIDTH;

const PIXELS_PER_CELL: i32 = 3;
const RESOLUTION: u32 = (MAP_CELLS * PIXELS_PER_CELL) as u32;
const REFRESH_SECS: f32 = 0.2;
const WATER: [u8; 3] = [38, 78, 104];
const OBSTACLE: [u8; 3] = [34, 62, 30];
const CRYSTAL: [u8; 3] = [90, 230, 210];
const VIEW_FRAME: [u8; 3] = [240, 240, 240];

#[derive(Resource)]
pub struct Minimap {
    pub image: Handle<Image>,
    terrain: Vec<u8>,
    refresh_timer: f32,
}

pub fn create_minimap(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    map: Res<GameMap>,
) {
    let ground = GroundLayers::new();
    let mut terrain = Vec::with_capacity((RESOLUTION * RESOLUTION * 4) as usize);
    for py in 0..RESOLUTION {
        for px in 0..RESOLUTION {
            let world =
                (Vec2::new(px as f32, py as f32) + 0.5) * (CELL_SIZE / PIXELS_PER_CELL as f32);
            let cell = (world / CELL_SIZE).floor().as_ivec2();
            let rgb = if river_depth(world.x / CELL_SIZE, world.y / CELL_SIZE) > 0.25 {
                WATER
            } else if !map.is_passable(cell) {
                OBSTACLE
            } else {
                let color = ground.minimap_color(world, map.crystal_at(cell) > 0) * 255.0;
                [color.x as u8, color.y as u8, color.z as u8]
            };
            terrain.extend_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
        }
    }
    let mut image = Image::new(
        Extent3d {
            width: RESOLUTION,
            height: RESOLUTION,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        terrain.clone(),
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::all(),
    );
    image.sampler = ImageSampler::nearest();
    commands.insert_resource(Minimap {
        image: images.add(image),
        terrain,
        refresh_timer: 0.0,
    });
}

struct Canvas<'a> {
    pixels: &'a mut [u8],
}

impl Canvas<'_> {
    fn put(&mut self, pixel: IVec2, rgb: [u8; 3]) {
        let inside = pixel.x >= 0
            && pixel.y >= 0
            && pixel.x < RESOLUTION as i32
            && pixel.y < RESOLUTION as i32;
        if inside {
            let at = ((pixel.y as u32 * RESOLUTION + pixel.x as u32) * 4) as usize;
            self.pixels[at..at + 3].copy_from_slice(&rgb);
        }
    }

    fn fill(&mut self, min: IVec2, size: IVec2, rgb: [u8; 3]) {
        for y in 0..size.y {
            for x in 0..size.x {
                self.put(min + IVec2::new(x, y), rgb);
            }
        }
    }

    fn line(&mut self, from: Vec2, to: Vec2, rgb: [u8; 3]) {
        let steps = from.distance(to).ceil().max(1.0) as i32;
        for step in 0..=steps {
            self.put(
                from.lerp(to, step as f32 / steps as f32).round().as_ivec2(),
                rgb,
            );
        }
    }
}

fn to_minimap(world: Vec3) -> Vec2 {
    world.xz() / CELL_SIZE * PIXELS_PER_CELL as f32
}

fn team_rgb(team: Team) -> [u8; 3] {
    let color = team.ui_color().to_srgba();
    [
        (color.red * 255.0) as u8,
        (color.green * 255.0) as u8,
        (color.blue * 255.0) as u8,
    ]
}

pub fn redraw_minimap(
    time: Res<Time>,
    map: Res<GameMap>,
    mut minimap: ResMut<Minimap>,
    mut images: ResMut<Assets<Image>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform), With<RtsCamera>>,
    buildings: Query<(&Building, &Team)>,
    units: Query<(&Transform, &Team), With<Unit>>,
) {
    minimap.refresh_timer -= time.delta_secs();
    if minimap.refresh_timer > 0.0 {
        return;
    }
    minimap.refresh_timer = REFRESH_SECS;
    let Some(mut image) = images.get_mut(&minimap.image) else {
        return;
    };
    let Some(pixels) = image.data.as_mut() else {
        return;
    };
    pixels.copy_from_slice(&minimap.terrain);
    let mut canvas = Canvas { pixels };

    for cell_y in 0..MAP_CELLS {
        for cell_x in 0..MAP_CELLS {
            let cell = IVec2::new(cell_x, cell_y);
            if map.crystal_at(cell) > 0 {
                canvas.fill(cell * PIXELS_PER_CELL + 1, IVec2::ONE, CRYSTAL);
            }
        }
    }
    for (building, &team) in &buildings {
        for cell in footprint_cells(building.origin, building.kind.spec().footprint) {
            canvas.fill(
                cell * PIXELS_PER_CELL,
                IVec2::splat(PIXELS_PER_CELL),
                team_rgb(team),
            );
        }
    }
    for (transform, &team) in &units {
        let center = to_minimap(transform.translation).floor().as_ivec2();
        canvas.fill(center - IVec2::ONE, IVec2::splat(2), team_rgb(team));
    }

    let (Ok(window), Ok((camera, camera_transform))) = (windows.single(), cameras.single()) else {
        return;
    };
    let playfield = Vec2::new(window.width() - SIDEBAR_WIDTH, window.height());
    let corners = [
        Vec2::ZERO,
        Vec2::new(playfield.x, 0.0),
        playfield,
        Vec2::new(0.0, playfield.y),
    ];
    let view: Option<Vec<Vec2>> = corners
        .iter()
        .map(|&screen| {
            let ray = camera.viewport_to_world(camera_transform, screen).ok()?;
            let hit = ray_hit_on_terrain(ray)
                .clamp(Vec3::ZERO, Vec3::splat(MAP_WORLD_SIZE).with_y(f32::MAX));
            Some(to_minimap(hit))
        })
        .collect();
    if let Some(view) = view {
        for i in 0..view.len() {
            canvas.line(view[i], view[(i + 1) % view.len()], VIEW_FRAME);
        }
    }
}

pub fn jump_camera_on_press(press: On<PointerPress>, mut cameras: Query<&mut RtsCamera>) {
    if press.button == PointerButton::Primary {
        jump_camera_to(&press.hit, &mut cameras);
    }
}

pub fn jump_camera_on_drag(
    movement: On<PointerMove>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut cameras: Query<&mut RtsCamera>,
) {
    if mouse.pressed(MouseButton::Left) {
        jump_camera_to(&movement.hit, &mut cameras);
    }
}

/// UI picking reports hits relative to the node, from (-0.5, -0.5) top left to (0.5, 0.5) bottom right.
fn jump_camera_to(hit: &HitData, cameras: &mut Query<&mut RtsCamera>) {
    let Some(position) = hit.position else { return };
    let normalized = (position.truncate() + Vec2::splat(0.5)).clamp(Vec2::ZERO, Vec2::ONE);
    let target = normalized * MAP_WORLD_SIZE;
    for mut camera in cameras.iter_mut() {
        camera.focus.x = target.x;
        camera.focus.z = target.y;
    }
}
