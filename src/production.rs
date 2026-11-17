use std::collections::VecDeque;

use bevy::prelude::*;
use rand::RngExt;
use serde::{Deserialize, Serialize};

use crate::assets::ModelLibrary;
use crate::camera::Cursor;
use crate::catalog::{BuildingKind, Team, UnitKind};
use crate::components::*;
use crate::economy::Economies;
use crate::map::{
    CELL_SIZE, GameMap, cell_center, footprint_cells, footprint_center, world_to_cell,
};
use crate::movement::order_move;
use crate::notifications::Notifications;
use crate::spawn::{exit_cell, spawn_building, spawn_unit};

pub const MAX_QUEUE_LENGTH: usize = 8;
/// How many cells of empty ground may separate a new structure from an existing one.
const BUILD_RADIUS_CELLS: i32 = 4;
const RALLY_DISTANCE_CELLS: i32 = 3;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum BuildItem {
    Structure(BuildingKind),
    Unit(UnitKind),
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub enum Lane {
    Structures,
    Infantry,
    Vehicles,
}

impl Lane {
    pub const ALL: [Lane; 3] = [Lane::Structures, Lane::Infantry, Lane::Vehicles];

    fn index(self) -> usize {
        match self {
            Lane::Structures => 0,
            Lane::Infantry => 1,
            Lane::Vehicles => 2,
        }
    }
}

impl BuildItem {
    pub fn name(self) -> &'static str {
        match self {
            BuildItem::Structure(kind) => kind.spec().name,
            BuildItem::Unit(kind) => kind.spec().name,
        }
    }

    pub fn cost(self) -> i32 {
        match self {
            BuildItem::Structure(kind) => kind.spec().cost,
            BuildItem::Unit(kind) => kind.spec().cost,
        }
    }

    pub fn build_secs(self) -> f32 {
        match self {
            BuildItem::Structure(kind) => kind.spec().build_secs,
            BuildItem::Unit(kind) => kind.spec().build_secs,
        }
    }

    pub fn lane(self) -> Lane {
        match self {
            BuildItem::Structure(_) => Lane::Structures,
            BuildItem::Unit(kind) if kind.spec().producer == BuildingKind::Barracks => {
                Lane::Infantry
            }
            BuildItem::Unit(_) => Lane::Vehicles,
        }
    }

    pub fn missing_requirement(self, owned: &[BuildingKind]) -> Option<BuildingKind> {
        let required: &[BuildingKind] = match self {
            BuildItem::Structure(kind) => kind.spec().prerequisites,
            BuildItem::Unit(kind) => std::slice::from_ref(&kind.spec().producer),
        };
        required.iter().copied().find(|kind| !owned.contains(kind))
    }
}

/// Why an item could not be added to a production queue.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EnqueueRefusal {
    NotInMission,
    Requires(BuildingKind),
    QueueFull,
    InsufficientFunds,
}

impl EnqueueRefusal {
    pub fn message(self) -> String {
        match self {
            EnqueueRefusal::NotInMission => "Not available in this mission".into(),
            EnqueueRefusal::Requires(missing) => format!("Requires {}", missing.spec().name),
            EnqueueRefusal::QueueFull => "Production queue is full".into(),
            EnqueueRefusal::InsufficientFunds => "Insufficient funds".into(),
        }
    }
}

#[derive(Default)]
pub struct ProductionLane {
    pub queue: VecDeque<BuildItem>,
    pub progress_secs: f32,
    /// A finished structure waiting to be placed on the map.
    pub ready: Option<BuildingKind>,
}

impl ProductionLane {
    pub fn is_idle(&self) -> bool {
        self.queue.is_empty() && self.ready.is_none()
    }

    pub fn progress_fraction(&self) -> f32 {
        self.queue.front().map_or(0.0, |item| {
            (self.progress_secs / item.build_secs()).min(1.0)
        })
    }
}

#[derive(Resource, Default)]
pub struct Production {
    lanes: [[ProductionLane; 3]; 4],
    /// Per team, the items the current mission does not allow.
    forbidden: [Vec<BuildItem>; 4],
}

impl Production {
    pub fn forbid(&mut self, team: Team, items: Vec<BuildItem>) {
        self.forbidden[team.index()] = items;
    }

    pub fn allows(&self, team: Team, item: BuildItem) -> bool {
        !self.forbidden[team.index()].contains(&item)
    }

    pub fn lane(&self, team: Team, lane: Lane) -> &ProductionLane {
        &self.lanes[team.index()][lane.index()]
    }

    pub fn lane_mut(&mut self, team: Team, lane: Lane) -> &mut ProductionLane {
        &mut self.lanes[team.index()][lane.index()]
    }

    pub fn enqueue(
        &mut self,
        economies: &mut Economies,
        team: Team,
        item: BuildItem,
        owned: &[BuildingKind],
    ) -> Result<(), EnqueueRefusal> {
        if !self.allows(team, item) {
            return Err(EnqueueRefusal::NotInMission);
        }
        if let Some(missing) = item.missing_requirement(owned) {
            return Err(EnqueueRefusal::Requires(missing));
        }
        let lane = self.lane_mut(team, item.lane());
        if lane.queue.len() >= MAX_QUEUE_LENGTH {
            return Err(EnqueueRefusal::QueueFull);
        }
        let economy = economies.of_mut(team);
        if economy.credits < item.cost() {
            return Err(EnqueueRefusal::InsufficientFunds);
        }
        economy.credits -= item.cost();
        lane.queue.push_back(item);
        Ok(())
    }

    /// Cancels the most recently queued copy of `item`, or the structure waiting to be placed.
    pub fn cancel(&mut self, economies: &mut Economies, team: Team, item: BuildItem) {
        let lane_kind = item.lane();
        let latest = self
            .lane(team, lane_kind)
            .queue
            .iter()
            .rposition(|&queued| queued == item);
        if let Some(position) = latest {
            self.cancel_at(economies, team, lane_kind, position);
        } else if let BuildItem::Structure(kind) = item
            && self.lane(team, lane_kind).ready == Some(kind)
        {
            self.cancel_ready_structure(economies, team);
        }
    }

    pub fn cancel_at(
        &mut self,
        economies: &mut Economies,
        team: Team,
        lane_kind: Lane,
        position: usize,
    ) {
        let lane = self.lane_mut(team, lane_kind);
        let Some(item) = lane.queue.remove(position) else {
            return;
        };
        if position == 0 {
            lane.progress_secs = 0.0;
        }
        economies.of_mut(team).credits += item.cost();
    }

    pub fn cancel_ready_structure(&mut self, economies: &mut Economies, team: Team) {
        let lane = self.lane_mut(team, Lane::Structures);
        if let Some(kind) = lane.ready.take() {
            economies.of_mut(team).credits += kind.spec().cost;
        }
    }
}

pub fn owned_building_kinds(
    buildings: &Query<(&Building, &Team)>,
    team: Team,
) -> Vec<BuildingKind> {
    buildings
        .iter()
        .filter(|(_, owner)| **owner == team)
        .map(|(building, _)| building.kind)
        .collect()
}

pub fn advance_production(
    mut commands: Commands,
    time: Res<Time>,
    models: Res<ModelLibrary>,
    map: Res<GameMap>,
    economies: Res<Economies>,
    mut production: ResMut<Production>,
    mut notifications: ResMut<Notifications>,
    buildings: Query<(&Building, &Team)>,
) {
    for team in Team::ALL {
        let rate = economies.of(team).production_rate();
        for lane_kind in Lane::ALL {
            let lane = production.lane_mut(team, lane_kind);
            let Some(&item) = lane.queue.front() else {
                continue;
            };
            if lane.ready.is_some() {
                continue;
            }
            let producer = match item {
                BuildItem::Structure(_) => None,
                BuildItem::Unit(kind) => {
                    let producer = buildings.iter().find(|(building, owner)| {
                        **owner == team && building.kind == kind.spec().producer
                    });
                    if producer.is_none() {
                        continue;
                    }
                    producer.map(|(building, _)| building)
                }
            };
            lane.progress_secs += time.delta_secs() * rate;
            if lane.progress_secs < item.build_secs() {
                continue;
            }
            lane.queue.pop_front();
            lane.progress_secs = 0.0;
            if team == Team::Player {
                notifications.push(match item {
                    BuildItem::Structure(_) => "Construction complete".to_string(),
                    BuildItem::Unit(kind) => format!("{} ready", kind.spec().name),
                });
            }
            match (item, producer) {
                (BuildItem::Structure(kind), _) => lane.ready = Some(kind),
                (BuildItem::Unit(kind), Some(producer)) => {
                    deploy_unit(&mut commands, &models, &map, kind, team, producer);
                }
                (BuildItem::Unit(_), None) => {}
            }
        }
    }
}

fn deploy_unit(
    commands: &mut Commands,
    models: &ModelLibrary,
    map: &GameMap,
    kind: UnitKind,
    team: Team,
    producer: &Building,
) -> Entity {
    let exit = map
        .nearest_passable(exit_cell(producer))
        .unwrap_or(exit_cell(producer));
    let entity = spawn_unit(commands, models, kind, team, cell_center(exit), 0.0);
    let mut rng = rand::rng();
    let rally = exit + IVec2::new(RALLY_DISTANCE_CELLS, rng.random_range(-2..=2));
    let mut mobile = Mobile::new(kind.spec().speed, kind.spec().turn_rate, 0.0);
    if order_move(&mut mobile, map, cell_center(exit), rally, false) {
        commands.entity(entity).insert(mobile);
    }
    entity
}

pub fn construct_building(
    commands: &mut Commands,
    models: &ModelLibrary,
    map: &mut GameMap,
    kind: BuildingKind,
    team: Team,
    origin: IVec2,
) {
    spawn_building(commands, models, map, kind, team, origin);
    if kind == BuildingKind::Refinery {
        let building = Building { kind, origin };
        let dock = map
            .nearest_passable(exit_cell(&building))
            .unwrap_or(exit_cell(&building));
        spawn_unit(
            commands,
            models,
            UnitKind::Harvester,
            team,
            cell_center(dock),
            0.0,
        );
    }
}

pub struct PlacementContext<'a> {
    pub map: &'a GameMap,
    pub own_buildings: Vec<(IVec2, IVec2)>,
    pub occupied_cells: Vec<IVec2>,
}

impl PlacementContext<'_> {
    pub fn can_place(&self, kind: BuildingKind, origin: IVec2) -> bool {
        let footprint = kind.spec().footprint;
        let ground_clear = footprint_cells(origin, footprint)
            .all(|cell| self.map.is_buildable(cell) && !self.occupied_cells.contains(&cell));
        ground_clear
            && self
                .own_buildings
                .iter()
                .any(|&(other_origin, other_footprint)| {
                    cell_gap(origin, footprint, other_origin, other_footprint) <= BUILD_RADIUS_CELLS
                })
    }
}

fn cell_gap(a_origin: IVec2, a_size: IVec2, b_origin: IVec2, b_size: IVec2) -> i32 {
    let a_end = a_origin + a_size;
    let b_end = b_origin + b_size;
    let gap_x = (b_origin.x - a_end.x).max(a_origin.x - b_end.x).max(0);
    let gap_y = (b_origin.y - a_end.y).max(a_origin.y - b_end.y).max(0);
    gap_x.max(gap_y)
}

pub fn placement_context<'a>(
    map: &'a GameMap,
    team: Team,
    buildings: &Query<(&Building, &Team)>,
    units: &Query<&Transform, With<Unit>>,
) -> PlacementContext<'a> {
    PlacementContext {
        map,
        own_buildings: buildings
            .iter()
            .filter(|(_, owner)| **owner == team)
            .map(|(building, _)| (building.origin, building.kind.spec().footprint))
            .collect(),
        occupied_cells: units
            .iter()
            .map(|transform| world_to_cell(transform.translation))
            .collect(),
    }
}

#[derive(Resource, Default)]
pub struct Placement {
    pub active: Option<BuildingKind>,
    ghost: Option<Entity>,
}

#[derive(Component)]
pub struct PlacementGhost;

#[derive(Resource)]
pub struct GhostMaterials {
    valid: Handle<StandardMaterial>,
    invalid: Handle<StandardMaterial>,
}

pub fn create_ghost_materials(
    mut commands: Commands,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let ghost = |color: Color| StandardMaterial {
        base_color: color,
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        ..default()
    };
    commands.insert_resource(GhostMaterials {
        valid: materials.add(ghost(Color::srgba(0.2, 1.0, 0.35, 0.35))),
        invalid: materials.add(ghost(Color::srgba(1.0, 0.15, 0.1, 0.4))),
    });
}

fn placement_origin(cursor_ground: Vec3, footprint: IVec2) -> IVec2 {
    let centered = cursor_ground
        - Vec3::new(footprint.x as f32 - 1.0, 0.0, footprint.y as f32 - 1.0) * CELL_SIZE * 0.5;
    world_to_cell(centered)
}

pub fn place_structure(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    cursor: Res<Cursor>,
    models: Res<ModelLibrary>,
    ghost_materials: Res<GhostMaterials>,
    mut map: ResMut<GameMap>,
    mut placement: ResMut<Placement>,
    mut production: ResMut<Production>,
    buildings: Query<(&Building, &Team)>,
    units: Query<&Transform, With<Unit>>,
    mut ghosts: Query<
        (&mut Transform, &mut MeshMaterial3d<StandardMaterial>),
        (With<PlacementGhost>, Without<Unit>),
    >,
) {
    let Some(kind) = placement.active else {
        if let Some(ghost) = placement.ghost.take() {
            commands.entity(ghost).despawn();
        }
        return;
    };
    if mouse.just_pressed(MouseButton::Right) || keys.just_pressed(KeyCode::Escape) {
        placement.active = None;
        return;
    }
    let footprint = kind.spec().footprint;
    let ghost = *placement.ghost.get_or_insert_with(|| {
        let size = footprint.as_vec2() * CELL_SIZE;
        commands
            .spawn((
                PlacementGhost,
                Mesh3d(meshes.add(Cuboid::new(size.x, 0.4, size.y))),
                MeshMaterial3d(ghost_materials.invalid.clone()),
                Transform::default(),
            ))
            .id()
    });
    let Some(ground) = cursor.ground.filter(|_| !cursor.over_interface) else {
        return;
    };
    let origin = placement_origin(ground, footprint);
    let valid = placement_context(&map, Team::Player, &buildings, &units).can_place(kind, origin);
    if let Ok((mut transform, mut material)) = ghosts.get_mut(ghost) {
        transform.translation = footprint_center(origin, footprint) + Vec3::Y * 0.2;
        material.0 = if valid {
            ghost_materials.valid.clone()
        } else {
            ghost_materials.invalid.clone()
        };
    }
    if valid && mouse.just_pressed(MouseButton::Left) {
        construct_building(&mut commands, &models, &mut map, kind, Team::Player, origin);
        production.lane_mut(Team::Player, Lane::Structures).ready = None;
        placement.active = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rich() -> Economies {
        let mut economies = Economies::default();
        economies.of_mut(Team::Player).credits = 100_000;
        economies
    }

    const BASE: [BuildingKind; 3] = [
        BuildingKind::CommandCenter,
        BuildingKind::PowerPlant,
        BuildingKind::Barracks,
    ];

    #[test]
    fn enqueueing_charges_and_cancelling_refunds() {
        let mut economies = Economies::default();
        let mut production = Production::default();
        let start = economies.of(Team::Player).credits;
        let rifleman = BuildItem::Unit(UnitKind::Rifleman);
        assert!(
            production
                .enqueue(&mut economies, Team::Player, rifleman, &BASE)
                .is_ok()
        );
        assert_eq!(economies.of(Team::Player).credits, start - rifleman.cost());
        production.cancel(&mut economies, Team::Player, rifleman);
        assert_eq!(economies.of(Team::Player).credits, start);
        assert!(
            production
                .lane(Team::Player, Lane::Infantry)
                .queue
                .is_empty()
        );
    }

    #[test]
    fn missing_prerequisites_and_poverty_block_production() {
        let mut production = Production::default();
        let mut economies = rich();
        let tank = BuildItem::Unit(UnitKind::Tank);
        assert_eq!(tank.missing_requirement(&BASE), Some(BuildingKind::Factory));
        assert_eq!(
            production.enqueue(&mut economies, Team::Player, tank, &BASE),
            Err(EnqueueRefusal::Requires(BuildingKind::Factory))
        );

        let mut poor = Economies::default();
        poor.of_mut(Team::Player).credits = 50;
        assert_eq!(
            production.enqueue(
                &mut poor,
                Team::Player,
                BuildItem::Unit(UnitKind::Rifleman),
                &BASE
            ),
            Err(EnqueueRefusal::InsufficientFunds)
        );
    }

    #[test]
    fn items_withheld_by_the_mission_are_refused_with_that_reason() {
        let mut production = Production::default();
        let mut economies = rich();
        let turret = BuildItem::Structure(BuildingKind::GunTurret);
        production.forbid(Team::Player, vec![turret]);
        assert_eq!(
            production.enqueue(&mut economies, Team::Player, turret, &BASE),
            Err(EnqueueRefusal::NotInMission)
        );
        assert_eq!(
            EnqueueRefusal::NotInMission.message(),
            "Not available in this mission"
        );
    }

    #[test]
    fn queues_hold_at_most_the_limit() {
        let mut production = Production::default();
        let mut economies = rich();
        let plant = BuildItem::Structure(BuildingKind::PowerPlant);
        let accepted = (0..MAX_QUEUE_LENGTH + 3)
            .filter(|_| {
                production
                    .enqueue(&mut economies, Team::Player, plant, &BASE)
                    .is_ok()
            })
            .count();
        assert_eq!(accepted, MAX_QUEUE_LENGTH);
    }

    #[test]
    fn cancelling_the_item_in_progress_resets_its_progress() {
        let mut production = Production::default();
        let mut economies = rich();
        let item = BuildItem::Unit(UnitKind::Rifleman);
        production
            .enqueue(&mut economies, Team::Player, item, &BASE)
            .unwrap();
        production
            .enqueue(&mut economies, Team::Player, item, &BASE)
            .unwrap();
        production
            .lane_mut(Team::Player, Lane::Infantry)
            .progress_secs = 3.0;
        production.cancel_at(&mut economies, Team::Player, Lane::Infantry, 0);
        let lane = production.lane(Team::Player, Lane::Infantry);
        assert_eq!(lane.queue.len(), 1);
        assert_eq!(lane.progress_secs, 0.0);
    }

    #[test]
    fn a_finished_structure_waiting_for_placement_can_be_refunded() {
        let mut production = Production::default();
        let mut economies = Economies::default();
        let before = economies.of(Team::Player).credits;
        production.lane_mut(Team::Player, Lane::Structures).ready = Some(BuildingKind::PowerPlant);
        production.cancel_ready_structure(&mut economies, Team::Player);
        assert_eq!(
            economies.of(Team::Player).credits,
            before + BuildingKind::PowerPlant.spec().cost
        );
        assert!(
            production
                .lane(Team::Player, Lane::Structures)
                .ready
                .is_none()
        );
    }

    #[test]
    fn structures_must_be_placed_close_to_the_base_on_free_ground() {
        let mut map = GameMap::open_field();
        let command_center = (IVec2::new(10, 10), IVec2::splat(3));
        map.set_building(
            command_center.0,
            command_center.1,
            Some(Entity::PLACEHOLDER),
        );
        let context = PlacementContext {
            map: &map,
            own_buildings: vec![command_center],
            occupied_cells: vec![IVec2::new(16, 10)],
        };
        assert!(context.can_place(BuildingKind::PowerPlant, IVec2::new(14, 10)));
        assert!(
            !context.can_place(BuildingKind::PowerPlant, IVec2::new(11, 11)),
            "overlaps a building"
        );
        assert!(
            !context.can_place(BuildingKind::PowerPlant, IVec2::new(15, 10)),
            "covers a unit"
        );
        assert!(
            !context.can_place(BuildingKind::PowerPlant, IVec2::new(30, 30)),
            "too far from the base"
        );
    }

    #[test]
    fn cell_gap_counts_empty_cells_between_footprints() {
        assert_eq!(
            cell_gap(IVec2::ZERO, IVec2::ONE, IVec2::new(1, 0), IVec2::ONE),
            0
        );
        assert_eq!(
            cell_gap(IVec2::ZERO, IVec2::splat(2), IVec2::new(5, 0), IVec2::ONE),
            3
        );
        assert_eq!(
            cell_gap(IVec2::ZERO, IVec2::splat(2), IVec2::new(4, 7), IVec2::ONE),
            5
        );
    }
}
