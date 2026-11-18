use std::cell::Cell;

use bevy::prelude::*;
use pathfinding::prelude::astar;
use rand::rngs::StdRng;
use rand::{RngExt, SeedableRng};
use serde::{Deserialize, Serialize};

use crate::animation::AnimatedModel;
use crate::assets::ModelLibrary;
use crate::catalog::CRYSTAL_PER_CELL;
use crate::fog_view::FogTexture;
use crate::levels::ActiveLevel;
use crate::scenery;
use crate::terrain::{
    FAR_RING_WIDTH, Landscape, OUTLANDS_WIDTH, WATER_LEVEL, approximate_ground,
    build_far_terrain_mesh, build_terrain_mesh,
};
use crate::terrain_material::{TerrainSurface, WaterMaterial, WaterSurface, water_material};

pub const CELL_SIZE: f32 = 2.0;
/// The map every landscape, road and crystal field is authored for; bigger maps spread them out.
pub const STANDARD_MAP_CELLS: i32 = 64;
/// Base sites and crystal fields closer than this to a standard base site keep their offset from it.
const BASE_SURROUNDINGS_CELLS: f32 = 12.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum MapSize {
    #[default]
    Standard,
    Large,
    Huge,
}

impl MapSize {
    pub const ALL: [MapSize; 3] = [MapSize::Standard, MapSize::Large, MapSize::Huge];

    pub fn cells(self) -> i32 {
        match self {
            MapSize::Standard => STANDARD_MAP_CELLS,
            MapSize::Large => 96,
            MapSize::Huge => 128,
        }
    }
}

// The size of the map being played. Cell maths runs everywhere, so it is ambient rather than passed
// around; it is kept per thread because every schedule runs on the thread that updates the app, and tests
// that play several battles at once each run on their own thread.
thread_local! {
    static ACTIVE_MAP_CELLS: Cell<i32> = const { Cell::new(STANDARD_MAP_CELLS) };
}

pub fn set_map_size(size: MapSize) {
    ACTIVE_MAP_CELLS.set(size.cells());
}

pub fn map_cells() -> i32 {
    ACTIVE_MAP_CELLS.get()
}

pub fn map_world_size() -> f32 {
    map_cells() as f32 * CELL_SIZE
}

/// How much farther apart things are than on the standard map.
pub fn map_spread() -> f32 {
    (map_cells() - 1) as f32 / (STANDARD_MAP_CELLS - 1) as f32
}

/// Command Center origins of the four corner base sites; levels decide who starts where.
pub fn base_sites() -> [IVec2; 4] {
    let far = map_cells();
    [
        IVec2::new(6, far - 12),
        IVec2::new(far - 10, 8),
        IVec2::new(6, 8),
        IVec2::new(far - 10, far - 12),
    ]
}

pub fn player_base_cell() -> IVec2 {
    base_sites()[0]
}

pub fn enemy_base_cell() -> IVec2 {
    base_sites()[1]
}

fn standard_base_sites() -> [IVec2; 4] {
    let far = STANDARD_MAP_CELLS;
    [
        IVec2::new(6, far - 12),
        IVec2::new(far - 10, 8),
        IVec2::new(6, 8),
        IVec2::new(far - 10, far - 12),
    ]
}

/// Moves a point in cell units, authored for the standard map, onto the map being played: near a base it
/// keeps its offset from that base, elsewhere it spreads out from the centre. Mirror twins stay twins.
pub fn relocate(standard: Vec2) -> Vec2 {
    let base_middle = |site: IVec2| site.as_vec2() + Vec2::splat(1.5);
    let nearest_base = standard_base_sites()
        .into_iter()
        .zip(base_sites())
        .map(|(old, new)| (base_middle(old), base_middle(new)))
        .min_by(|a, b| a.0.distance(standard).total_cmp(&b.0.distance(standard)));
    if let Some((old, new)) = nearest_base
        && old.distance(standard) < BASE_SURROUNDINGS_CELLS
    {
        return new + (standard - old);
    }
    let old_middle = Vec2::splat((STANDARD_MAP_CELLS - 1) as f32 / 2.0);
    let new_middle = Vec2::splat((map_cells() - 1) as f32 / 2.0);
    new_middle + (standard - old_middle) * map_spread()
}

/// A cell and its mirror image always add up to this, so the map plays the same from opposite corners.
fn mirror_sum() -> i32 {
    map_cells() - 2
}

pub fn opposite_cell(cell: IVec2) -> IVec2 {
    IVec2::splat(mirror_sum()) - cell
}

/// Where a structure of `footprint` starting at `origin` lands when the map is mirrored.
pub fn opposite_origin(origin: IVec2, footprint: IVec2) -> IVec2 {
    opposite_cell(origin) - (footprint - IVec2::ONE)
}

/// Structures here are laid out and turned as the mirror image of their twin in the near half.
pub fn in_far_half(origin: IVec2, footprint: IVec2) -> bool {
    let opposite = opposite_origin(origin, footprint);
    origin.x + origin.y > opposite.x + opposite.y
}

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
        let cells = (map_cells() * map_cells()) as usize;
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
        let size = map_cells();
        let inside = cell.x >= 0 && cell.y >= 0 && cell.x < size && cell.y < size;
        inside.then(|| (cell.y * size + cell.x) as usize)
    }

    pub fn in_bounds(cell: IVec2) -> bool {
        Self::index(cell).is_some()
    }

    pub fn is_passable(&self, cell: IVec2) -> bool {
        Self::index(cell).is_some_and(|i| !self.terrain_blocked[i] && self.building[i].is_none())
    }

    pub fn building_at(&self, cell: IVec2) -> Option<Entity> {
        Self::index(cell).and_then(|i| self.building[i])
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

    pub fn set_crystal(&mut self, cell: IVec2, amount: u32) {
        if let Some(i) = Self::index(cell) {
            self.crystal[i] = amount;
        }
    }

    pub fn set_obstacle(&mut self, cell: IVec2, blocked: bool) {
        if let Some(i) = Self::index(cell) {
            self.terrain_blocked[i] = blocked;
        }
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
        cells_by_distance(from, map_cells()).find(|&cell| self.is_passable(cell))
    }

    /// Routes starting in the far half are planned in mirror image, so tie-breaks between equally short
    /// routes favour neither corner.
    pub fn find_path(&self, start: IVec2, goal: IVec2) -> Option<Vec<IVec2>> {
        let starts_in_far_half = start.x + start.y > mirror_sum();
        if !starts_in_far_half {
            return self.find_path_in_frame(start, goal, |cell| cell);
        }
        let path =
            self.find_path_in_frame(opposite_cell(start), opposite_cell(goal), opposite_cell)?;
        Some(path.into_iter().map(opposite_cell).collect())
    }

    /// A* over cells of a frame whose cell `c` is the real map cell `to_map(c)`.
    fn find_path_in_frame(
        &self,
        start: IVec2,
        goal: IVec2,
        to_map: impl Fn(IVec2) -> IVec2,
    ) -> Option<Vec<IVec2>> {
        let passable = |cell: IVec2| self.is_passable(to_map(cell));
        let goal = if passable(goal) {
            goal
        } else {
            cells_by_distance(goal, map_cells()).find(|&cell| passable(cell))?
        };
        let (path, _cost) = astar(
            &start,
            |&cell| walkable_neighbours(cell, passable),
            |&cell| octile_distance(cell, goal),
            |&cell| cell == goal,
        )?;
        Some(path)
    }
}

fn octile_distance(a: IVec2, b: IVec2) -> u32 {
    let d = (a - b).abs();
    let (short, long) = (d.x.min(d.y) as u32, d.x.max(d.y) as u32);
    DIAGONAL_COST * short + CARDINAL_COST * (long - short)
}

/// Ring by ring outward from `center`, nearest rings first, staying inside the map.
fn walkable_neighbours(cell: IVec2, passable: impl Fn(IVec2) -> bool) -> Vec<(IVec2, u32)> {
    let mut neighbours = Vec::with_capacity(8);
    for dy in -1..=1 {
        for dx in -1..=1 {
            if dx == 0 && dy == 0 {
                continue;
            }
            let next = cell + IVec2::new(dx, dy);
            if !passable(next) {
                continue;
            }
            let diagonal = dx != 0 && dy != 0;
            let cuts_corner = diagonal
                && (!passable(cell + IVec2::new(dx, 0)) || !passable(cell + IVec2::new(0, dy)));
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
    approximate_ground(Vec3::new(
        (cell.x as f32 + 0.5) * CELL_SIZE,
        0.0,
        (cell.y as f32 + 0.5) * CELL_SIZE,
    ))
}

pub fn footprint_center(origin: IVec2, footprint: IVec2) -> Vec3 {
    approximate_ground(Vec3::new(
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

/// On the standard map, in mirrored pairs: every even entry is followed by its opposite.
const STANDARD_CRYSTAL_FIELDS: [CrystalField; 6] = [
    CrystalField {
        center: IVec2::new(15, 46),
        radius: 3.6,
    },
    CrystalField {
        center: IVec2::new(47, 16),
        radius: 3.6,
    },
    CrystalField {
        center: IVec2::new(24, 38),
        radius: 2.6,
    },
    CrystalField {
        center: IVec2::new(38, 24),
        radius: 2.6,
    },
    CrystalField {
        center: IVec2::new(6, 22),
        radius: 3.2,
    },
    CrystalField {
        center: IVec2::new(56, 40),
        radius: 3.2,
    },
];

/// The crystal fields placed on the map being played, still in mirrored pairs.
pub fn crystal_fields() -> Vec<CrystalField> {
    STANDARD_CRYSTAL_FIELDS
        .iter()
        .map(|field| CrystalField {
            center: relocate(field.center.as_vec2() + Vec2::splat(0.5))
                .floor()
                .as_ivec2(),
            radius: field.radius,
        })
        .collect()
}

pub fn near_base(cell: IVec2) -> bool {
    let base_clearance = 9.0;
    base_sites()
        .iter()
        .any(|base| (cell - (*base + IVec2::ONE)).as_vec2().length() < base_clearance)
}

pub fn near_crystal_field(cell: IVec2) -> bool {
    crystal_fields()
        .iter()
        .any(|field| (cell - field.center).as_vec2().length() < field.radius + 2.0)
}

pub fn generate_map(mut commands: Commands, models: Res<ModelLibrary>, level: Res<ActiveLevel>) {
    let mut rng = StdRng::seed_from_u64(level.0.map_seed);
    let mut map = GameMap::empty();
    let landscape = level.0.landscape;
    block_water(&mut map, landscape);
    seed_crystal_fields(&mut commands, &models, &mut map, &mut rng);
    scenery::scatter(&mut commands, &models, landscape, &mut map, &mut rng);
    commands.insert_resource(map);
}

pub fn is_water(landscape: Landscape, cell: IVec2) -> bool {
    landscape.water_depth(cell.x as f32 + 0.5, cell.y as f32 + 0.5) > RIVER_BLOCKING_DEPTH
}

fn block_water(map: &mut GameMap, landscape: Landscape) {
    let size = map_cells();
    for row in 0..size {
        for column in 0..size {
            if is_water(landscape, IVec2::new(column, row)) {
                map.terrain_blocked[(row * size + column) as usize] = true;
            }
        }
    }
}

/// The ground inside the map, whose soil shows where crystals grow; the far ring around it never changes.
#[derive(Component)]
pub struct PlayfieldGround;

pub fn playfield_ground_mesh(landscape: Landscape, map: &GameMap) -> Mesh {
    build_terrain_mesh(landscape, |cell| map.crystal_at(cell) > 0)
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
    level: Res<ActiveLevel>,
) {
    commands.spawn((
        TerrainSurface,
        PlayfieldGround,
        Mesh3d(meshes.add(playfield_ground_mesh(level.0.landscape, &map))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.30, 0.36, 0.20),
            perceptual_roughness: 0.95,
            ..default()
        })),
        Transform::IDENTITY,
    ));

    commands.spawn((
        TerrainSurface,
        Mesh3d(meshes.add(build_far_terrain_mesh(level.0.landscape))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.30, 0.36, 0.20),
            perceptual_roughness: 0.95,
            ..default()
        })),
        Transform::IDENTITY,
    ));

    let water_span = map_world_size() + 2.0 * (OUTLANDS_WIDTH + FAR_RING_WIDTH);
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
        Transform::from_xyz(map_world_size() * 0.5, WATER_LEVEL, map_world_size() * 0.5),
    ));
}

fn seed_crystal_fields(
    commands: &mut Commands,
    models: &ModelLibrary,
    map: &mut GameMap,
    rng: &mut StdRng,
) {
    for field in crystal_fields().iter().step_by(2) {
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
            let Some(mirror) = GameMap::index(opposite_cell(cell)) else {
                continue;
            };
            let richness = 1.0 - 0.5 * (distance / field.radius);
            let amount = (CRYSTAL_PER_CELL as f32 * richness) as u32;
            for (index, at) in [(i, cell), (mirror, opposite_cell(cell))] {
                map.crystal[index] = amount;
                spawn_crystal_visual(commands, models, rng, at, amount);
            }
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
    IVec2::new(index as i32 % map_cells(), index as i32 / map_cells())
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
        block_water(&mut map, Landscape::River);
        map
    }

    fn is_ford_column(column: i32) -> bool {
        [(9, 13), (29, 33), (49, 53)]
            .iter()
            .any(|&(start, end)| column >= start && column <= end)
    }

    #[test]
    fn bases_are_connected_and_the_river_is_crossed_only_at_fords() {
        let map = map_with_river();
        let path = map
            .find_path(player_base_cell(), enemy_base_cell())
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
    fn opposite_base_sites_mirror_each_other() {
        let command_center = IVec2::splat(3);
        assert_eq!(
            opposite_origin(base_sites()[0], command_center),
            base_sites()[1]
        );
        assert_eq!(
            opposite_origin(base_sites()[2], command_center),
            base_sites()[3]
        );
    }

    #[test]
    fn the_river_and_crystal_fields_are_point_symmetric() {
        let map = map_with_river();
        for cell in (0..map_cells()).flat_map(|y| (0..map_cells()).map(move |x| IVec2::new(x, y))) {
            let mirror = opposite_cell(cell);
            if GameMap::in_bounds(mirror) {
                assert_eq!(
                    map.is_passable(cell),
                    map.is_passable(mirror),
                    "{cell} vs {mirror}"
                );
            }
        }
        for pair in crystal_fields().chunks(2) {
            assert_eq!(opposite_cell(pair[0].center), pair[1].center);
            assert_eq!(pair[0].radius, pair[1].radius);
        }
    }

    #[test]
    fn every_landscape_connects_the_bases_and_mirrors_its_water() {
        for landscape in Landscape::ALL {
            let mut map = GameMap::empty();
            block_water(&mut map, landscape);
            for (from, to) in [
                (base_sites()[0], base_sites()[1]),
                (base_sites()[2], base_sites()[3]),
            ] {
                assert!(
                    map.find_path(from, to).is_some(),
                    "{landscape:?}: {from} cut off from {to}"
                );
            }
            for cell in
                (0..map_cells()).flat_map(|y| (0..map_cells()).map(move |x| IVec2::new(x, y)))
            {
                let mirror = opposite_cell(cell);
                if GameMap::in_bounds(mirror) {
                    assert_eq!(
                        map.is_passable(cell),
                        map.is_passable(mirror),
                        "{landscape:?} at {cell}"
                    );
                }
            }
        }
    }

    #[test]
    fn the_lake_blocks_the_middle_except_for_its_causeway() {
        let mut map = GameMap::empty();
        block_water(&mut map, Landscape::Lake);
        let middle = IVec2::splat(31);
        assert!(map.is_passable(middle), "the causeway is flooded");
        assert!(
            !map.is_passable(middle + IVec2::new(4, 4)),
            "no water beside the causeway"
        );
        for field in &crystal_fields() {
            assert!(
                map.is_passable(field.center),
                "crystals under water at {}",
                field.center
            );
        }
    }

    #[test]
    fn mirrored_trips_take_mirrored_routes() {
        let map = map_with_river();
        let start = IVec2::new(11, 53);
        for goal in [IVec2::new(55, 9), IVec2::new(7, 9), IVec2::new(40, 20)] {
            let route = map.find_path(start, goal).unwrap();
            let mirrored = map
                .find_path(opposite_cell(start), opposite_cell(goal))
                .unwrap();
            assert_eq!(
                route.into_iter().map(opposite_cell).collect::<Vec<_>>(),
                mirrored
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
        for cell in [
            IVec2::ZERO,
            IVec2::new(17, 42),
            IVec2::splat(map_cells() - 1),
        ] {
            assert_eq!(world_to_cell(cell_center(cell)), cell);
            assert_eq!(cell_of_index(GameMap::index(cell).unwrap()), cell);
        }
    }
}
