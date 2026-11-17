use std::f32::consts::TAU;
use std::ops::Range;

use bevy::prelude::*;
use rand::RngExt;
use rand::rngs::StdRng;

use crate::ambience::{Sway, Trampleable};
use crate::assets::ModelLibrary;
use crate::components::Building;
use crate::graphics::FineDecor;
use crate::map::{
    CELL_SIZE, GameMap, MAP_CELLS, MAP_WORLD_SIZE, cell_center, near_base, near_crystal_field,
};
use crate::terrain::{
    GroundLayers, OUTLANDS_WIDTH, river_centerline, terrain_height, terrain_normal,
};

const FOREST_COUNT: usize = 16;
const CONIFER_FOREST: [&str; 3] = ["tree_pine", "tree_pine", "tree_pine_young"];
const DECIDUOUS_FOREST: [&str; 4] = ["tree_broadleaf", "tree", "tree_birch", "tree_broadleaf"];
const MIXED_FOREST: [&str; 5] = [
    "tree_pine",
    "tree_broadleaf",
    "tree_birch",
    "tree",
    "tree_pine_young",
];
const DEAD_TREE_CHANCE: f64 = 0.05;
const BUSHES: [&str; 3] = ["bush", "bush_round", "bush_flowering"];
const ROCKS: [&str; 3] = ["rock", "rock_a", "rock_b"];
const MEADOW_ATTEMPTS: usize = 2600;
const CLUMP_SIZE: std::ops::Range<usize> = 2..6;
const CLUMP_SPREAD: f32 = 0.9;
const RIVERBANK_REED_ATTEMPTS: usize = 420;
const OUTLAND_TREE_ATTEMPTS: usize = 1100;
const OUTLAND_BOULDER_COUNT: usize = 90;
const MAX_MEADOW_DIRT: f32 = 0.35;

/// Any static piece of landscape: trees, rocks and decoration.
#[derive(Component)]
pub struct Scenery;

/// Walk-through decoration that is cleared away when a structure is built on top of it.
#[derive(Component)]
pub struct Decor;

pub fn scatter(
    commands: &mut Commands,
    models: &ModelLibrary,
    map: &mut GameMap,
    rng: &mut StdRng,
) {
    let ground = GroundLayers::new();
    plant_forests(commands, models, map, rng);
    place_rocks(commands, models, map, rng);
    dress_meadows(commands, models, map, rng, &ground);
    line_riverbanks(commands, models, map, rng);
    cover_mountains(commands, models, rng);
}

fn pick<'a>(rng: &mut StdRng, models: &[&'a str]) -> &'a str {
    models[rng.random_range(0..models.len())]
}

fn plant_forests(
    commands: &mut Commands,
    models: &ModelLibrary,
    map: &mut GameMap,
    rng: &mut StdRng,
) {
    for _ in 0..FOREST_COUNT {
        let center = IVec2::new(
            rng.random_range(2..MAP_CELLS - 2),
            rng.random_range(2..MAP_CELLS - 2),
        );
        let species: &[&str] = match rng.random_range(0..3) {
            0 => &CONIFER_FOREST,
            1 => &DECIDUOUS_FOREST,
            _ => &MIXED_FOREST,
        };
        for _ in 0..rng.random_range(6..14) {
            let cell = center + IVec2::new(rng.random_range(-3..=3), rng.random_range(-3..=3));
            let model = if rng.random_bool(DEAD_TREE_CHANCE) {
                "tree_dead"
            } else {
                pick(rng, species)
            };
            block_with_prop(commands, models, map, rng, cell, model, 0.8..1.25);
        }
        for _ in 0..rng.random_range(4..9) {
            let offset = Vec3::new(
                rng.random_range(-8.0..8.0),
                0.0,
                rng.random_range(-8.0..8.0),
            );
            let position = cell_center(center) + offset;
            let undergrowth = match rng.random_range(0..5) {
                0 => "stump",
                1 => "fallen_log",
                _ => pick(rng, &BUSHES),
            };
            if is_open_ground(map, position) {
                place_decor(
                    commands,
                    models,
                    rng,
                    undergrowth,
                    position,
                    0.8..1.2,
                    false,
                );
            }
        }
    }
}

fn place_rocks(
    commands: &mut Commands,
    models: &ModelLibrary,
    map: &mut GameMap,
    rng: &mut StdRng,
) {
    for _ in 0..40 {
        let cell = IVec2::new(
            rng.random_range(0..MAP_CELLS),
            rng.random_range(0..MAP_CELLS),
        );
        let model = pick(rng, &ROCKS);
        block_with_prop(commands, models, map, rng, cell, model, 0.6..1.3);
    }
}

fn dress_meadows(
    commands: &mut Commands,
    models: &ModelLibrary,
    map: &GameMap,
    rng: &mut StdRng,
    ground: &GroundLayers,
) {
    for _ in 0..MEADOW_ATTEMPTS {
        let position = Vec3::new(
            rng.random_range(0.0..MAP_WORLD_SIZE),
            0.0,
            rng.random_range(0.0..MAP_WORLD_SIZE),
        );
        let weights = ground.weights(position.xz(), 0.0);
        let grassy =
            weights.x < MAX_MEADOW_DIRT && weights.y < 0.3 && weights.z < 0.05 && weights.w < 0.05;
        if !grassy || !is_open_ground(map, position) {
            continue;
        }
        let (model, scale) = match rng.random_range(0..100) {
            0..=71 => ("grass_tuft", 0.8..1.5),
            72..=83 => ("flowers", 0.8..1.2),
            84..=92 => ("pebbles", 0.7..1.3),
            _ => (pick(rng, &BUSHES), 0.7..1.2),
        };
        let clump = if model == "grass_tuft" || model == "flowers" {
            rng.random_range(CLUMP_SIZE)
        } else {
            1
        };
        for _ in 0..clump {
            let offset = Vec3::new(
                rng.random_range(-CLUMP_SPREAD..CLUMP_SPREAD),
                0.0,
                rng.random_range(-CLUMP_SPREAD..CLUMP_SPREAD),
            );
            place_decor(
                commands,
                models,
                rng,
                model,
                position + offset,
                scale.clone(),
                model == "pebbles",
            );
        }
    }
}

fn line_riverbanks(
    commands: &mut Commands,
    models: &ModelLibrary,
    map: &GameMap,
    rng: &mut StdRng,
) {
    for _ in 0..RIVERBANK_REED_ATTEMPTS {
        let column = rng.random_range(
            -OUTLANDS_WIDTH / CELL_SIZE..MAP_CELLS as f32 + OUTLANDS_WIDTH / CELL_SIZE,
        );
        let side = if rng.random_bool(0.5) { 1.0 } else { -1.0 };
        let row = river_centerline(column) + side * rng.random_range(1.5..2.4);
        let position = Vec3::new(column * CELL_SIZE, 0.0, row * CELL_SIZE);
        let inside_map = GameMap::in_bounds((position.xz() / CELL_SIZE).floor().as_ivec2());
        if inside_map && !is_open_ground(map, position) {
            continue;
        }
        place_decor(commands, models, rng, "reeds", position, 0.8..1.3, false);
    }
}

fn cover_mountains(commands: &mut Commands, models: &ModelLibrary, rng: &mut StdRng) {
    let span = -OUTLANDS_WIDTH..MAP_WORLD_SIZE + OUTLANDS_WIDTH;
    let random_outland_point = |rng: &mut StdRng| loop {
        let position = Vec3::new(
            rng.random_range(span.clone()),
            0.0,
            rng.random_range(span.clone()),
        );
        let inside = (0.0..MAP_WORLD_SIZE).contains(&position.x)
            && (0.0..MAP_WORLD_SIZE).contains(&position.z);
        if !inside {
            return position;
        }
    };
    for _ in 0..OUTLAND_TREE_ATTEMPTS {
        let position = random_outland_point(rng);
        let height = terrain_height(position.x, position.z);
        let too_steep = terrain_normal(position.x, position.z).y < 0.8;
        if too_steep || height < 0.2 || height > 13.0 {
            continue;
        }
        let model = if height > 5.0 {
            pick(rng, &CONIFER_FOREST)
        } else if rng.random_bool(0.1) {
            "tree_dead"
        } else {
            pick(rng, &MIXED_FOREST)
        };
        place_decor(commands, models, rng, model, position, 0.9..1.5, false);
    }
    for _ in 0..OUTLAND_BOULDER_COUNT {
        let position = random_outland_point(rng);
        place_decor(
            commands,
            models,
            rng,
            "boulder_large",
            position,
            1.0..3.0,
            true,
        );
    }
}

fn is_open_ground(map: &GameMap, position: Vec3) -> bool {
    let cell = (position.xz() / CELL_SIZE).floor().as_ivec2();
    map.is_buildable(cell) && !near_base(cell) && !near_crystal_field(cell)
}

fn block_with_prop(
    commands: &mut Commands,
    models: &ModelLibrary,
    map: &mut GameMap,
    rng: &mut StdRng,
    cell: IVec2,
    model_id: &str,
    scale: Range<f32>,
) {
    let free = map.is_buildable(cell) && !near_base(cell) && !near_crystal_field(cell);
    let Some(i) = GameMap::index(cell).filter(|_| free) else {
        return;
    };
    map.terrain_blocked[i] = true;
    let jitter = Vec3::new(
        rng.random_range(-0.3..0.3),
        0.0,
        rng.random_range(-0.3..0.3),
    );
    spawn_scenery(
        commands,
        models,
        rng,
        model_id,
        cell_center(cell) + jitter,
        scale,
        false,
    );
}

fn place_decor(
    commands: &mut Commands,
    models: &ModelLibrary,
    rng: &mut StdRng,
    model_id: &str,
    position: Vec3,
    scale: Range<f32>,
    follow_slope: bool,
) {
    let entity = spawn_scenery(
        commands,
        models,
        rng,
        model_id,
        position,
        scale,
        follow_slope,
    );
    commands.entity(entity).insert(Decor);
}

fn spawn_scenery(
    commands: &mut Commands,
    models: &ModelLibrary,
    rng: &mut StdRng,
    model_id: &str,
    position: Vec3,
    scale: Range<f32>,
    follow_slope: bool,
) -> Entity {
    let ground = Vec3::new(
        position.x,
        terrain_height(position.x, position.z),
        position.z,
    );
    let heading = Quat::from_rotation_y(rng.random_range(0.0..TAU));
    let rotation = if follow_slope {
        Quat::from_rotation_arc(Vec3::Y, terrain_normal(ground.x, ground.z)) * heading
    } else {
        heading
    };
    let mut scenery = commands.spawn((
        Scenery,
        WorldAssetRoot(models.neutral(model_id)),
        Transform::from_translation(ground)
            .with_rotation(rotation)
            .with_scale(Vec3::splat(rng.random_range(scale))),
    ));
    if matches!(model_id, "grass_tuft" | "flowers" | "pebbles") {
        scenery.insert(FineDecor);
    }
    if is_trampleable(model_id) {
        scenery.insert(Trampleable::default());
    }
    if let Some(amplitude) = sway_amplitude(model_id) {
        scenery.insert(Sway {
            rest: rotation,
            phase: rng.random_range(0.0..TAU),
            amplitude,
            speed: rng.random_range(1.2..2.0),
        });
    }
    scenery.id()
}

fn is_trampleable(model_id: &str) -> bool {
    matches!(
        model_id,
        "grass_tuft" | "flowers" | "reeds" | "bush" | "bush_round" | "bush_flowering"
    )
}

/// How far each kind of vegetation leans in the wind, in radians; rigid scenery returns `None`.
fn sway_amplitude(model_id: &str) -> Option<f32> {
    match model_id {
        "reeds" => Some(0.09),
        "bush" | "bush_round" | "bush_flowering" => Some(0.035),
        "tree_birch" | "tree_pine_young" => Some(0.03),
        "tree" | "tree_pine" | "tree_broadleaf" => Some(0.018),
        _ => None,
    }
}

pub fn clear_decor_under_new_buildings(
    mut commands: Commands,
    new_buildings: Query<&Building, Added<Building>>,
    decor: Query<(Entity, &Transform), With<Decor>>,
) {
    let clearing_margin = 0.6;
    for building in &new_buildings {
        let min = building.origin.as_vec2() * CELL_SIZE - Vec2::splat(clearing_margin);
        let max = (building.origin + building.kind.spec().footprint).as_vec2() * CELL_SIZE
            + Vec2::splat(clearing_margin);
        for (entity, transform) in &decor {
            let point = transform.translation.xz();
            if point.cmpge(min).all() && point.cmple(max).all() {
                commands.entity(entity).despawn();
            }
        }
    }
}
