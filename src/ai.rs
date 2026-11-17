use bevy::prelude::*;
use rand::RngExt;
use serde::{Deserialize, Serialize};

use crate::assets::ModelLibrary;
use crate::catalog::{BuildingKind, Team, UnitKind};
use crate::combat::{TargetInfo, Targets};
use crate::components::*;
use crate::economy::Economies;
use crate::fog::FogOfWar;
use crate::intel::Intel;
use crate::levels::Difficulty;
use crate::map::{
    CELL_SIZE, GameMap, cells_by_distance, footprint_cells, footprint_center, in_far_half,
    opposite_cell, opposite_origin, world_to_cell,
};
use crate::production::{
    BuildItem, Lane, PlacementContext, Production, construct_building, owned_building_kinds,
    placement_context,
};
use crate::structures::{BeingSold, Repairing, StructureOrder, StructureOrderKind};

const MAX_WAVE_SIZE: usize = 16;
const WAVE_GROWTH: usize = 2;
/// With no enemy soldier in sight for this long, whatever waits at home goes hunting.
const QUIET_FRONT_SECS: f32 = 90.0;
const BASE_DEFENSE_RADIUS: f32 = 16.0 * CELL_SIZE;
const SITE_SEARCH_RADIUS: i32 = 14;
const CRYSTAL_SEARCH_RADIUS: i32 = 20;
const HARVESTERS_PER_REFINERY: usize = 2;
const COMBAT_UNIT_RESERVE: i32 = 400;
const REPAIR_BELOW_HEALTH: f32 = 0.6;

const BUILD_PLAN: [BuildingKind; 13] = [
    BuildingKind::PowerPlant,
    BuildingKind::Barracks,
    BuildingKind::Refinery,
    BuildingKind::PowerPlant,
    BuildingKind::Factory,
    BuildingKind::GunTurret,
    BuildingKind::PowerPlant,
    BuildingKind::GunTurret,
    BuildingKind::Refinery,
    BuildingKind::PowerPlant,
    BuildingKind::GunTurret,
    BuildingKind::PowerPlant,
    BuildingKind::GunTurret,
];

/// One computer opponent: its team, home base and attack rhythm.
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct AiCommander {
    pub team: Team,
    pub base: IVec2,
    pub difficulty: Difficulty,
    think_timer: f32,
    wave_size: usize,
    #[serde(default)]
    secs_since_enemy_army_seen: f32,
    #[serde(skip)]
    scout: Option<Entity>,
}

impl AiCommander {
    pub fn new(team: Team, base: IVec2, difficulty: Difficulty) -> Self {
        Self {
            team,
            base,
            difficulty,
            think_timer: difficulty.think_interval_secs(),
            wave_size: difficulty.first_wave_size(),
            secs_since_enemy_army_seen: 0.0,
            scout: None,
        }
    }
}

#[derive(Resource, Clone, Default, Serialize, Deserialize, Debug)]
pub struct AiCommanders(pub Vec<AiCommander>);

type ArmyUnit<'a> = (
    Entity,
    &'a Unit,
    &'a Team,
    &'a Transform,
    &'a mut Mobile,
    Option<&'a AttackTarget>,
    Option<&'a mut Orders>,
);

pub fn command_enemies(
    mut commands: Commands,
    time: Res<Time>,
    models: Res<ModelLibrary>,
    targets: Res<Targets>,
    intel: Res<Intel>,
    player_fog: Res<FogOfWar>,
    mut commanders: ResMut<AiCommanders>,
    mut map: ResMut<GameMap>,
    mut production: ResMut<Production>,
    mut economies: ResMut<Economies>,
    buildings: Query<(&Building, &Team)>,
    unit_positions: Query<&Transform, With<Unit>>,
    mut army: Query<ArmyUnit>,
) {
    for ai in commanders.0.iter_mut() {
        ai.think_timer -= time.delta_secs();
        if ai.think_timer > 0.0 {
            continue;
        }
        ai.think_timer = ai.difficulty.think_interval_secs();

        let mut owned = owned_building_kinds(&buildings, ai.team);
        if owned.is_empty() {
            continue;
        }
        let placed = place_finished_structure(
            ai,
            &mut commands,
            &models,
            &mut map,
            &mut production,
            &buildings,
            &unit_positions,
        );
        owned.extend(placed);
        let saving_for = plan_next_structure(ai.team, &mut production, &mut economies, &owned);
        let harvesters = army
            .iter()
            .filter(|(_, unit, team, ..)| **team == ai.team && unit.0 == UnitKind::Harvester)
            .count();
        train_units(
            ai.team,
            &mut production,
            &mut economies,
            &owned,
            harvesters,
            saving_for,
        );
        let knowledge = Knowledge {
            targets: &targets,
            intel: &intel,
            player_fog: &player_fog,
        };
        direct_army(ai, &knowledge, &buildings, &mut army);
    }
}

/// Computer opponents patch up badly damaged structures whenever they can spare the credits.
pub fn repair_damaged_structures(
    commanders: Res<AiCommanders>,
    economies: Res<Economies>,
    structures: Query<
        (Entity, &Team, &Health),
        (With<Building>, Without<Repairing>, Without<BeingSold>),
    >,
    mut orders: MessageWriter<StructureOrder>,
) {
    for (building, &team, health) in &structures {
        let commanded = commanders.0.iter().any(|ai| ai.team == team);
        let badly_damaged = health.current > 0.0 && health.fraction() < REPAIR_BELOW_HEALTH;
        let can_spare = economies.of(team).credits >= COMBAT_UNIT_RESERVE;
        if commanded && badly_damaged && can_spare {
            orders.write(StructureOrder {
                team,
                building,
                kind: StructureOrderKind::ToggleRepair,
            });
        }
    }
}

fn place_finished_structure(
    ai: &AiCommander,
    commands: &mut Commands,
    models: &ModelLibrary,
    map: &mut GameMap,
    production: &mut Production,
    buildings: &Query<(&Building, &Team)>,
    unit_positions: &Query<&Transform, With<Unit>>,
) -> Option<BuildingKind> {
    let kind = production.lane(ai.team, Lane::Structures).ready?;
    let site = {
        let context = placement_context(map, ai.team, buildings, unit_positions);
        find_building_site(&context, ai.base, kind)
    };
    let origin = site?;
    construct_building(commands, models, map, kind, ai.team, origin);
    production.lane_mut(ai.team, Lane::Structures).ready = None;
    Some(kind)
}

/// A free spot near `base` that leaves a one-cell lane around the structure so units never get boxed in.
pub fn find_building_site(
    context: &PlacementContext,
    base: IVec2,
    kind: BuildingKind,
) -> Option<IVec2> {
    let footprint = kind.spec().footprint;
    let frame = BaseFrame::of(base);
    let home = frame.origin(base, BuildingKind::CommandCenter.spec().footprint);
    let search_center = if kind == BuildingKind::Refinery {
        towards_nearest_crystal(context.map, home, frame)
    } else {
        home
    };
    let mut candidates = cells_by_distance(search_center, SITE_SEARCH_RADIUS)
        .map(|origin| frame.origin(origin, footprint));
    candidates.find(|&origin| {
        let margin_origin = origin - IVec2::ONE;
        let keeps_lanes_open = footprint_cells(margin_origin, footprint + IVec2::splat(2))
            .all(|cell| !GameMap::in_bounds(cell) || context.map.is_passable(cell));
        keeps_lanes_open && context.can_place(kind, origin)
    })
}

/// Bases in the far half of the map lay themselves out as the mirror image of their opposite number,
/// so search-order tie-breaks favour neither corner.
#[derive(Clone, Copy)]
struct BaseFrame {
    mirrored: bool,
}

impl BaseFrame {
    fn of(base: IVec2) -> Self {
        Self {
            mirrored: in_far_half(base, BuildingKind::CommandCenter.spec().footprint),
        }
    }

    fn cell(self, cell: IVec2) -> IVec2 {
        if self.mirrored {
            opposite_cell(cell)
        } else {
            cell
        }
    }

    fn origin(self, origin: IVec2, footprint: IVec2) -> IVec2 {
        if self.mirrored {
            opposite_origin(origin, footprint)
        } else {
            origin
        }
    }
}

/// Halfway to the closest crystals, so harvesters have a short haul; `home` is in the frame's coordinates.
fn towards_nearest_crystal(map: &GameMap, home: IVec2, frame: BaseFrame) -> IVec2 {
    cells_by_distance(home, CRYSTAL_SEARCH_RADIUS)
        .find(|&cell| map.crystal_at(frame.cell(cell)) > 0)
        .map_or(home, |crystal| (home + crystal) / 2)
}

/// Queues the next structure of the build plan; returns the price it is saving up for when it cannot pay yet.
fn plan_next_structure(
    team: Team,
    production: &mut Production,
    economies: &mut Economies,
    owned: &[BuildingKind],
) -> Option<i32> {
    if !production.lane(team, Lane::Structures).is_idle() {
        return None;
    }
    let mut planned_so_far = Vec::new();
    let next_in_plan = BUILD_PLAN
        .iter()
        .copied()
        .filter(|&kind| production.allows(team, BuildItem::Structure(kind)))
        .find(|&kind| {
            planned_so_far.push(kind);
            let wanted = planned_so_far.iter().filter(|&&k| k == kind).count();
            owned.iter().filter(|&&k| k == kind).count() < wanted
        });
    let mut next = next_in_plan?;

    let economy = economies.of(team);
    let spare_power = economy.power_supply - economy.power_demand;
    if next.spec().power < 0 && spare_power + next.spec().power < 0 {
        next = BuildingKind::PowerPlant;
    }
    let item = BuildItem::Structure(next);
    production
        .enqueue(economies, team, item, owned)
        .err()
        .map(|_| item.cost())
}

/// Weighted wish lists; anything the mission forbids is skipped.
const VEHICLE_MIX: [(UnitKind, u32); 4] = [
    (UnitKind::Tank, 5),
    (UnitKind::Bulwark, 2),
    (UnitKind::Artillery, 2),
    (UnitKind::Scout, 3),
];
const INFANTRY_MIX: [(UnitKind, u32); 3] = [
    (UnitKind::Rifleman, 6),
    (UnitKind::Rocketeer, 3),
    (UnitKind::Medic, 1),
];

fn pick_weighted(mix: &[(UnitKind, u32)], allowed: impl Fn(UnitKind) -> bool) -> Option<UnitKind> {
    let candidates: Vec<(UnitKind, u32)> = mix
        .iter()
        .copied()
        .filter(|&(kind, _)| allowed(kind))
        .collect();
    let total: u32 = candidates.iter().map(|(_, weight)| weight).sum();
    if total == 0 {
        return None;
    }
    let mut roll = rand::rng().random_range(0..total);
    candidates.into_iter().find_map(|(kind, weight)| {
        if roll < weight {
            Some(kind)
        } else {
            roll -= weight;
            None
        }
    })
}

fn train_units(
    team: Team,
    production: &mut Production,
    economies: &mut Economies,
    owned: &[BuildingKind],
    harvesters: usize,
    saving_for: Option<i32>,
) {
    let refineries = owned
        .iter()
        .filter(|&&k| k == BuildingKind::Refinery)
        .count();
    let vehicles_idle = production.lane(team, Lane::Vehicles).queue.is_empty();
    let credits = economies.of(team).credits;
    let stranded = harvesters == 0 && credits < UnitKind::Harvester.spec().cost;
    let reserve = if stranded {
        0
    } else {
        saving_for.unwrap_or(COMBAT_UNIT_RESERVE)
    };
    let affordable = |economies: &Economies, kind: UnitKind| {
        economies.of(team).credits >= kind.spec().cost + reserve
    };
    let permitted: Vec<UnitKind> = UnitKind::ALL
        .into_iter()
        .filter(|&kind| production.allows(team, BuildItem::Unit(kind)))
        .collect();
    let allowed = |kind: UnitKind| permitted.contains(&kind);

    if vehicles_idle {
        let wants_harvester = refineries > 0 && harvesters < refineries * HARVESTERS_PER_REFINERY;
        let choice = if wants_harvester {
            Some(UnitKind::Harvester)
        } else {
            pick_weighted(&VEHICLE_MIX, allowed).filter(|&kind| affordable(economies, kind))
        };
        if let Some(kind) = choice {
            let _ = production.enqueue(economies, team, BuildItem::Unit(kind), owned);
        }
    }
    let infantry_queue = production.lane(team, Lane::Infantry).queue.len();
    if infantry_queue < 2
        && let Some(kind) =
            pick_weighted(&INFANTRY_MIX, allowed).filter(|&kind| affordable(economies, kind))
    {
        let _ = production.enqueue(economies, team, BuildItem::Unit(kind), owned);
    }
}

/// What one commander knows when it plans: only enemies its side can see, and structures it has spotted.
struct Knowledge<'a> {
    targets: &'a Targets,
    intel: &'a Intel,
    player_fog: &'a FogOfWar,
}

fn direct_army(
    ai: &mut AiCommander,
    knowledge: &Knowledge,
    buildings: &Query<(&Building, &Team)>,
    army: &mut Query<ArmyUnit>,
) {
    let team = ai.team;
    let home = footprint_center(ai.base, BuildingKind::CommandCenter.spec().footprint);
    let base_positions: Vec<Vec3> = buildings
        .iter()
        .filter(|(_, owner)| **owner == team)
        .map(|(building, _)| footprint_center(building.origin, building.kind.spec().footprint))
        .collect();
    let near_base = |position: Vec3| {
        base_positions
            .iter()
            .any(|base| base.distance(position) < BASE_DEFENSE_RADIUS)
    };
    let visible_enemies: Vec<&TargetInfo> = knowledge
        .targets
        .iter()
        .filter(|t| t.team.is_hostile_to(team) && t.seen_by(team))
        .collect();
    let intruder = visible_enemies
        .iter()
        .find(|t| near_base(t.position))
        .map(|t| t.position);
    let known_objective = known_assault_objective(team, home, &visible_enemies, knowledge.intel);
    let search_site = || {
        knowledge
            .intel
            .base_site_to_search(team, knowledge.player_fog, home)
    };
    let assault_objective = known_objective.or_else(search_site);

    if visible_enemies.iter().any(|t| !t.is_building) {
        ai.secs_since_enemy_army_seen = 0.0;
    } else {
        ai.secs_since_enemy_army_seen += ai.difficulty.think_interval_secs();
    }
    let scout_alive = ai.scout.is_some_and(|scout| army.contains(scout));
    let needs_scout = known_objective.is_none() && !scout_alive;

    let mut waiting_at_home = Vec::new();
    for (entity, unit, &owner, transform, mobile, attack, orders) in army.iter_mut() {
        let idle = !mobile.is_moving()
            && attack.is_none_or(|a| a.target.is_none())
            && orders.as_ref().is_none_or(|o| o.current.is_none());
        let is_soldier = owner == team && unit.0 != UnitKind::Harvester;
        let Some(orders) = orders.filter(|_| is_soldier && idle) else {
            continue;
        };
        let at_home = near_base(transform.translation);
        let objective = if at_home {
            intruder
        } else {
            intruder.or(assault_objective)
        };
        match objective {
            Some(objective) => send(orders, objective),
            None if at_home => waiting_at_home.push((entity, unit.0, orders)),
            None => send(orders, home),
        }
    }

    if needs_scout && let Some(site) = search_site() {
        let pick = waiting_at_home
            .iter()
            .position(|(_, kind, _)| *kind == UnitKind::Scout)
            .or((!waiting_at_home.is_empty()).then_some(0));
        if let Some(index) = pick {
            let (scout, _, mut orders) = waiting_at_home.swap_remove(index);
            orders.replace_with(Order::Move(world_to_cell(site)));
            ai.scout = Some(scout);
        }
    }

    let wave_ready = waiting_at_home.len() >= ai.wave_size;
    let enemy_army_gone_quiet = ai.secs_since_enemy_army_seen > QUIET_FRONT_SECS;
    let finish_them_off = enemy_army_gone_quiet && !waiting_at_home.is_empty();
    if !wave_ready && !finish_them_off {
        return;
    }
    let Some(objective) = assault_objective else {
        return;
    };
    for (_, _, orders) in waiting_at_home {
        send(orders, objective);
    }
    ai.wave_size = (ai.wave_size + WAVE_GROWTH).min(MAX_WAVE_SIZE);
}

/// The nearest enemy the commander knows of, preferring structures, which do not run away.
fn known_assault_objective(
    team: Team,
    home: Vec3,
    visible_enemies: &[&TargetInfo],
    intel: &Intel,
) -> Option<Vec3> {
    const UNIT_DETOUR: f32 = 30.0;
    let units = visible_enemies
        .iter()
        .filter(|t| !t.is_building)
        .map(|t| (t.position, UNIT_DETOUR));
    let structures = intel
        .known_enemy_structures(team)
        .map(|position| (position, 0.0));
    units
        .chain(structures)
        .min_by(|(a, a_detour), (b, b_detour)| {
            (a.distance(home) + a_detour).total_cmp(&(b.distance(home) + b_detour))
        })
        .map(|(position, _)| position)
}

fn send(mut orders: Mut<Orders>, objective: Vec3) {
    orders.replace_with(Order::AttackMove(world_to_cell(objective)));
}
