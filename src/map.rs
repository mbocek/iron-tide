use bevy::prelude::*;
use pathfinding::prelude::astar;
use rand::rngs::StdRng;
use rand::{RngExt, SeedableRng};

use crate::animation::AnimatedModel;
use crate::assets::ModelLibrary;
use crate::catalog::CRYSTAL_PER_CELL;
use crate::fog_view::FogTexture;
use crate::levels::ActiveLevel;
use crate::scenery;
use crate::terrain::{OUTLANDS_WIDTH, WATER_LEVEL, build_terrain_mesh, on_ground, river_depth};
use crate::terrain_material::{TerrainSurface, WaterMaterial, WaterSurface, water_material};

pub const CELL_SIZE: f32 = 2.0;
pub const MAP_CELLS: i32 = 64;
pub const MAP_WORLD_SIZE: f32 = MAP_CELLS as f32 * CELL_SIZE;
/// Command Center origins of the four corner base sites; levels decide who starts where.
pub const BASE_SITES: [IVec2; 4] = [
    IVec2::new(6, 52),
    IVec2::new(54, 8),
    IVec2::new(6, 8),
    IVec2::new(54, 52),
];
pub const PLAYER_BASE_CELL: IVec2 = BASE_SITES[0];
pub const ENEMY_BASE_CELL: IVec2 = BASE_SITES[1];

const CRYSTAL_VARIANTS: [&str; 3] = ["crystal_a", "crystal_b", "crystal_c"];
const RIVER_BLOCKING_DEPTH: f32 = 0.25;

const CARDINAL_COST: u32 = 10;
const DIAGONAL_COST: u32 = 14;

#[derive(Resource)]
pub struct GameMap {
    pub(crate) terrain_blocked: Vec<bool>,
    building: Vec<Option<Entity>>,
    crystal: Vec<u32>,
}

#[derive(Component)]
pub struct CrystalVisual {
    pub cell: IVec2,
}

impl GameMap {
    fn empty() -> Self {
        let cells = (MAP_CELLS * MAP_CELLS) as usize;
        Self {
            terrain_blocked: vec![false; cells],
            building: vec![None; cells],
            crystal: vec![0; cells],
        }
    }

    /// A map with no river, crystals or obstacles, for tests.
    #[cfg(test)]
    pub(crate) fn open_field() -> Self {
        Self::empty()
    }

    pub(crate) fn index(cell: IVec2) -> Option<usize> {
        let inside = cell.x >= 0 && cell.y >= 0 && cell.x < MAP_CELLS && cell.y < MAP_CELLS;
        inside.then(|| (cell.y * MAP_CELLS + cell.x) as usize)
    }

    pub fn in_bounds(cell: IVec2) -> bool {
        Self::index(cell).is_some()
    }

    pub fn is_passable(&self, cell: IVec2) -> bool {
        Self::index(cell).is_some_and(|i| !self.terrain_blocked[i] && self.building[i].is_none())
    }

    pub fn is_buildable(&self, cell: IVec2) -> bool {
        self.is_passable(cell) && self.crystal_at(cell) == 0
    }

    pub fn set_building(&mut self, origin: IVec2, footprint: IVec2, occupant: Option<Entity>) {
        for cell in footprint_cells(origin, footprint) {
            if let Some(i) = Self::index(cell) {
                self.building[i] = occupant;
            }
        }
    }

    pub fn crystal_amounts(&self) -> Vec<u32> {
        self.crystal.clone()
    }

    /// Restores a saved crystal grid and forgets every building footprint, ready for buildings to be respawned.
    pub fn reset_for_load(&mut self, crystal: Vec<u32>) {
        if crystal.len() == self.crystal.len() {
            self.crystal = crystal;
        }
        self.building.fill(None);
    }

    pub fn crystal_at(&self, cell: IVec2) -> u32 {
        Self::index(cell).map_or(0, |i| self.crystal[i])
    }

    /// Returns the amount actually mined and whether the cell is now depleted.
    pub fn mine_crystal(&mut self, cell: IVec2, wanted: u32) -> (u32, bool) {
        let Some(i) = Self::index(cell) else {
            return (0, true);
        };
        let mined = wanted.min(self.crystal[i]);
        self.crystal[i] -= mined;
        (mined, self.crystal[i] == 0)
    }

    pub fn nearest_crystal(&self, from: IVec2, max_radius: i32) -> Option<IVec2> {
        cells_by_distance(from, max_radius).find(|&cell| self.crystal_at(cell) > 0)
    }

    pub fn nearest_passable(&self, from: IVec2) -> Option<IVec2> {
        cells_by_distance(from, MAP_CELLS).find(|&cell| self.is_passable(cell))
    }

    pub fn find_path(&self, start: IVec2, goal: IVec2) -> Option<Vec<IVec2>> {
        let goal = if self.is_passable(goal) {
            goal
        } else {
            self.nearest_passable(goal)?
        };
        let (path, _cost) = astar(
            &start,
            |&cell| self.walkable_neighbours(cell),
            |&cell| octile_distance(cell, goal),
            |&cell| cell == goal,
        )?;
        Some(path)
    }

    fn walkable_neighbours(&self, cell: IVec2) -> Vec<(IVec2, u32)> {
        let mut neighbours = Vec::with_capacity(8);
        for dy in -1..=1 {
            for dx in -1..=1 {
                if dx == 0 && dy == 0 {
                    continue;
                }
                let next = cell + IVec2::new(dx, dy);
                if !self.is_passable(next) {
                    continue;
                }
                let diagonal = dx != 0 && dy != 0;
                let cuts_corner = diagonal
                    && (!self.is_passable(cell + IVec2::new(dx, 0))
                        || !self.is_passable(cell + IVec2::new(0, dy)));
                if cuts_corner {
                    continue;
                }
                neighbours.push((
                    next,
                    if diagonal {
                        DIAGONAL_COST
                    } else {
                        CARDINAL_COST
                    },
                ));
            }
        }
        neighbours
    }
}

fn octile_distance(a: IVec2, b: IVec2) -> u32 {
    let d = (a - b).abs();
    let (short, long) = (d.x.min(d.y) as u32, d.x.max(d.y) as u32);
    DIAGONAL_COST * short + CARDINAL_COST * (long - short)
}

/// Ring by ring outward from `center`, nearest rings first, staying inside the map.
pub fn cells_by_distance(center: IVec2, max_radius: i32) -> impl Iterator<Item = IVec2> {
    (0..=max_radius).flat_map(move |radius| {
        (-radius..=radius).flat_map(move |dy| {
            (-radius..=radius).filter_map(move |dx| {
                let on_ring = dx.abs() == radius || dy.abs() == radius;
                let cell = center + IVec2::new(dx, dy);
                (on_ring && GameMap::in_bounds(cell)).then_some(cell)
            })
        })
    })
}

pub fn footprint_cells(origin: IVec2, footprint: IVec2) -> impl Iterator<Item = IVec2> {
    (0..footprint.y).flat_map(move |dy| (0..footprint.x).map(move |dx| origin + IVec2::new(dx, dy)))
}

pub fn cell_center(cell: IVec2) -> Vec3 {
    on_ground(Vec3::new(
        (cell.x as f32 + 0.5) * CELL_SIZE,
        0.0,
        (cell.y as f32 + 0.5) * CELL_SIZE,
    ))
}

pub fn footprint_center(origin: IVec2, footprint: IVec2) -> Vec3 {
    on_ground(Vec3::new(
        (origin.x as f32 + footprint.x as f32 * 0.5) * CELL_SIZE,
        0.0,
        (origin.y as f32 + footprint.y as f32 * 0.5) * CELL_SIZE,
    ))
}

pub fn world_to_cell(position: Vec3) -> IVec2 {
    IVec2::new(
        (position.x / CELL_SIZE).floor() as i32,
        (position.z / CELL_SIZE).floor() as i32,
    )
}

pub struct CrystalField {
    pub center: IVec2,
    pub radius: f32,
}

pub const CRYSTAL_FIELDS: [CrystalField; 6] = [
    CrystalField {
        center: IVec2::new(15, 46),
        radius: 3.6,
    },
    CrystalField {
        center: IVec2::new(45, 14),
        radius: 3.6,
    },
    CrystalField {
        center: IVec2::new(24, 38),
        radius: 2.6,
    },
    CrystalField {
        center: IVec2::new(36, 22),
        radius: 2.6,
    },
    CrystalField {
        center: IVec2::new(6, 22),
        radius: 3.2,
    },
    CrystalField {
        center: IVec2::new(54, 38),
        radius: 3.2,
    },
];

pub fn near_base(cell: IVec2) -> bool {
    let base_clearance = 9.0;
    BASE_SITES
        .iter()
        .any(|base| (cell - *base).as_vec2().length() < base_clearance)
}

pub fn near_crystal_field(cell: IVec2) -> bool {
    CRYSTAL_FIELDS
        .iter()
        .any(|field| (cell - field.center).as_vec2().length() < field.radius + 2.0)
}

pub fn generate_map(mut commands: Commands, models: Res<ModelLibrary>, level: Res<ActiveLevel>) {
    let mut rng = StdRng::seed_from_u64(level.0.map_seed);
    let mut map = GameMap::empty();
    block_river(&mut map);
    seed_crystal_fields(&mut commands, &models, &mut map, &mut rng);
    scenery::scatter(&mut commands, &models, &mut map, &mut rng);
    commands.insert_resource(map);
}

fn block_river(map: &mut GameMap) {
    for row in 0..MAP_CELLS {
        for column in 0..MAP_CELLS {
            let depth = river_depth(column as f32 + 0.5, row as f32 + 0.5);
            if depth > RIVER_BLOCKING_DEPTH {
                map.terrain_blocked[(row * MAP_CELLS + column) as usize] = true;
            }
        }
    }
}

/// The rendered ground and water; the simulation itself only needs the grid from `generate_map`.
pub fn spawn_terrain_visuals(
    mut commands: Commands,
    map: Res<GameMap>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut water_materials: ResMut<Assets<WaterMaterial>>,
    fog: Res<FogTexture>,
) {
    commands.spawn((
        TerrainSurface,
        Mesh3d(meshes.add(build_terrain_mesh(|cell| map.crystal_at(cell) > 0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.30, 0.36, 0.20),
            perceptual_roughness: 0.95,
            ..default()
        })),
        Transform::IDENTITY,
    ));

    let water_span = MAP_WORLD_SIZE + 2.0 * OUTLANDS_WIDTH;
    let water_mesh = Plane3d::default()
        .mesh()
        .size(water_span, water_span)
        .build()
        .with_generated_tangents()
        .expect("plane mesh has normals and UVs");
    commands.spawn((
        WaterSurface,
        Mesh3d(meshes.add(water_mesh)),
        MeshMaterial3d(water_materials.add(water_material(&mut images, fog.0.clone()))),
        Transform::from_xyz(MAP_WORLD_SIZE * 0.5, WATER_LEVEL, MAP_WORLD_SIZE * 0.5),
    ));
}

fn seed_crystal_fields(
    commands: &mut Commands,
    models: &ModelLibrary,
    map: &mut GameMap,
    rng: &mut StdRng,
) {
    for field in &CRYSTAL_FIELDS {
        let reach = field.radius.ceil() as i32;
        for cell in cells_by_distance(field.center, reach) {
            let distance = (cell - field.center).as_vec2().length();
            let jitter = rng.random_range(-0.6..0.6);
            let Some(i) = GameMap::index(cell) else {
                continue;
            };
            if distance + jitter > field.radius || map.terrain_blocked[i] {
                continue;
            }
            let richness = 1.0 - 0.5 * (distance / field.radius);
            map.crystal[i] = (CRYSTAL_PER_CELL as f32 * richness) as u32;
            spawn_crystal_visual(commands, models, rng, cell, map.crystal[i]);
        }
    }
}

pub fn spawn_crystal_visual(
    commands: &mut Commands,
    models: &ModelLibrary,
    rng: &mut impl RngExt,
    cell: IVec2,
    amount: u32,
) {
    let variant = CRYSTAL_VARIANTS[rng.random_range(0..CRYSTAL_VARIANTS.len())];
    commands.spawn((
        CrystalVisual { cell },
        WorldAssetRoot(models.neutral(variant)),
        AnimatedModel(variant.to_string()),
        Transform::from_translation(cell_center(cell))
            .with_rotation(Quat::from_rotation_y(
                rng.random_range(0.0..std::f32::consts::TAU),
            ))
            .with_scale(Vec3::splat(crystal_scale(amount))),
    ));
}

pub fn cell_of_index(index: usize) -> IVec2 {
    IVec2::new(index as i32 % MAP_CELLS, index as i32 / MAP_CELLS)
}

pub fn crystal_scale(amount: u32) -> f32 {
    0.45 + 0.6 * (amount as f32 / CRYSTAL_PER_CELL as f32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terrain::river_centerline;

    fn map_with_river() -> GameMap {
        let mut map = GameMap::empty();
        block_river(&mut map);
        map
    }

    fn is_ford_column(column: i32) -> bool {
        [(9, 13), (30, 34), (51, 55)]
            .iter()
            .any(|&(start, end)| column >= start && column <= end)
    }

    #[test]
    fn bases_are_connected_and_the_river_is_crossed_only_at_fords() {
        let map = map_with_river();
        let path = map
            .find_path(PLAYER_BASE_CELL, ENEMY_BASE_CELL)
            .expect("bases must be connected");
        assert!(path.iter().all(|&cell| map.is_passable(cell)));
        for cell in &path {
            let in_river_band = (cell.y as f32 - river_centerline(cell.x as f32 + 0.5)).abs() < 1.0;
            assert!(
                !in_river_band || is_ford_column(cell.x),
                "crossed the river outside a ford at {cell}"
            );
        }
    }

    #[test]
    fn path_never_cuts_diagonally_past_a_blocked_corner() {
        let mut map = GameMap::empty();
        map.set_building(IVec2::new(10, 10), IVec2::ONE, Some(Entity::PLACEHOLDER));
        let path = map
            .find_path(IVec2::new(9, 11), IVec2::new(11, 9))
            .expect("route around the obstacle");
        for step in path.windows(2) {
            let delta = step[1] - step[0];
            if delta.x != 0 && delta.y != 0 {
                assert!(map.is_passable(step[0] + IVec2::new(delta.x, 0)));
                assert!(map.is_passable(step[0] + IVec2::new(0, delta.y)));
            }
        }
    }

    #[test]
    fn path_to_a_blocked_goal_ends_next_to_it() {
        let mut map = GameMap::empty();
        let goal = IVec2::new(20, 20);
        map.set_building(goal, IVec2::splat(3), Some(Entity::PLACEHOLDER));
        let path = map
            .find_path(IVec2::new(5, 5), goal + IVec2::ONE)
            .expect("a reachable substitute goal");
        let end = *path.last().unwrap();
        assert!(map.is_passable(end));
        assert!((end - (goal + IVec2::ONE)).abs().max_element() <= 2);
    }

    #[test]
    fn buildings_block_cells_until_removed() {
        let mut map = GameMap::empty();
        let origin = IVec2::new(4, 4);
        map.set_building(origin, IVec2::splat(2), Some(Entity::PLACEHOLDER));
        assert!(footprint_cells(origin, IVec2::splat(2)).all(|cell| !map.is_passable(cell)));
        map.set_building(origin, IVec2::splat(2), None);
        assert!(footprint_cells(origin, IVec2::splat(2)).all(|cell| map.is_passable(cell)));
    }

    #[test]
    fn mining_takes_at_most_what_is_left_and_reports_depletion() {
        let mut map = GameMap::empty();
        let cell = IVec2::new(3, 3);
        map.crystal[GameMap::index(cell).unwrap()] = 30;
        assert_eq!(map.mine_crystal(cell, 25), (25, false));
        assert_eq!(map.mine_crystal(cell, 25), (5, true));
        assert_eq!(map.crystal_at(cell), 0);
        assert!(map.is_buildable(cell));
    }

    #[test]
    fn rings_are_visited_nearest_first_and_stay_on_the_map() {
        let cells: Vec<IVec2> = cells_by_distance(IVec2::ZERO, 3).collect();
        assert_eq!(cells[0], IVec2::ZERO);
        assert!(cells.iter().all(|&cell| GameMap::in_bounds(cell)));
        let rings: Vec<i32> = cells.iter().map(|cell| cell.abs().max_element()).collect();
        assert!(rings.windows(2).all(|pair| pair[0] <= pair[1]));
    }

    #[test]
    fn world_and_cell_coordinates_round_trip() {
        for cell in [IVec2::ZERO, IVec2::new(17, 42), IVec2::splat(MAP_CELLS - 1)] {
            assert_eq!(world_to_cell(cell_center(cell)), cell);
            assert_eq!(cell_of_index(GameMap::index(cell).unwrap()), cell);
        }
    }
}
