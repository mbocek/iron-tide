use bevy::prelude::*;
use rand::RngExt;
use serde::{Deserialize, Serialize};

use crate::assets::ModelLibrary;
use crate::catalog::{BuildingKind, Team, UnitKind};
use crate::combat::{TargetInfo, Targets};
use crate::components::*;
use crate::economy::Economies;
use crate::levels::Difficulty;
use crate::map::{
    CELL_SIZE, GameMap, cell_center, cells_by_distance, footprint_cells, footprint_center,
    world_to_cell,
};
use crate::movement::order_move;
use crate::production::{
    BuildItem, Lane, PlacementContext, Production, construct_building, owned_building_kinds,
    placement_context,
};

const MAX_WAVE_SIZE: usize = 16;
const BASE_DEFENSE_RADIUS: f32 = 16.0 * CELL_SIZE;
const SITE_SEARCH_RADIUS: i32 = 14;
const HARVESTERS_PER_REFINERY: usize = 2;
const COMBAT_UNIT_RESERVE: i32 = 400;

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
    assault_underway: bool,
}

impl AiCommander {
    pub fn new(team: Team, base: IVec2, difficulty: Difficulty) -> Self {
        Self {
            team,
            base,
            difficulty,
            think_timer: difficulty.think_interval_secs(),
            wave_size: difficulty.first_wave_size(),
            assault_underway: false,
        }
    }
}

#[derive(Resource, Clone, Default, Serialize, Deserialize, Debug)]
pub struct AiCommanders(pub Vec<AiCommander>);

type ArmyUnit<'a> = (
    &'a Unit,
    &'a Team,
    &'a Transform,
    &'a mut Mobile,
    Option<&'a AttackTarget>,
);

pub fn command_enemies(
    mut commands: Commands,
    time: Res<Time>,
    models: Res<ModelLibrary>,
    targets: Res<Targets>,
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

        let owned = owned_building_kinds(&buildings, ai.team);
        if owned.is_empty() {
            continue;
        }
        place_finished_structure(
            ai,
            &mut commands,
            &models,
            &mut map,
            &mut production,
            &buildings,
            &unit_positions,
        );
        plan_next_structure(ai.team, &mut production, &mut economies, &owned);
        let harvesters = army
            .iter()
            .filter(|(unit, team, ..)| **team == ai.team && unit.0 == UnitKind::Harvester)
            .count();
        train_units(ai.team, &mut production, &mut economies, &owned, harvesters);
        direct_army(ai, &map, &targets, &buildings, &mut army);
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
) {
    let Some(kind) = production.lane(ai.team, Lane::Structures).ready else {
        return;
    };
    let site = {
        let context = placement_context(map, ai.team, buildings, unit_positions);
        find_building_site(&context, ai.base, kind)
    };
    if let Some(origin) = site {
        construct_building(commands, models, map, kind, ai.team, origin);
        production.lane_mut(ai.team, Lane::Structures).ready = None;
    }
}

/// A free spot near `base` that leaves a one-cell lane around the structure so units never get boxed in.
pub fn find_building_site(
    context: &PlacementContext,
    base: IVec2,
    kind: BuildingKind,
) -> Option<IVec2> {
    let footprint = kind.spec().footprint;
    cells_by_distance(base, SITE_SEARCH_RADIUS).find(|&origin| {
        let margin_origin = origin - IVec2::ONE;
        let keeps_lanes_open = footprint_cells(margin_origin, footprint + IVec2::splat(2))
            .all(|cell| !GameMap::in_bounds(cell) || context.map.is_passable(cell));
        keeps_lanes_open && context.can_place(kind, origin)
    })
}

fn plan_next_structure(
    team: Team,
    production: &mut Production,
    economies: &mut Economies,
    owned: &[BuildingKind],
) {
    if !production.lane(team, Lane::Structures).is_idle() {
        return;
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
    let Some(mut next) = next_in_plan else { return };

    let economy = economies.of(team);
    let spare_power = economy.power_supply - economy.power_demand;
    if next.spec().power < 0 && spare_power + next.spec().power < 0 {
        next = BuildingKind::PowerPlant;
    }
    let _ = production.enqueue(economies, team, BuildItem::Structure(next), owned);
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
) {
    let refineries = owned
        .iter()
        .filter(|&&k| k == BuildingKind::Refinery)
        .count();
    let vehicles_idle = production.lane(team, Lane::Vehicles).queue.is_empty();
    let structure_budget_reserved = !production.lane(team, Lane::Structures).is_idle();
    let reserve = if structure_budget_reserved {
        0
    } else {
        COMBAT_UNIT_RESERVE
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

fn direct_army(
    ai: &mut AiCommander,
    map: &GameMap,
    targets: &Targets,
    buildings: &Query<(&Building, &Team)>,
    army: &mut Query<ArmyUnit>,
) {
    let team = ai.team;
    let is_soldier = |unit: &Unit, owner: Team| owner == team && unit.0 != UnitKind::Harvester;
    let soldiers = army
        .iter()
        .filter(|(unit, owner, ..)| is_soldier(unit, **owner))
        .count();
    let idle_soldiers = army
        .iter()
        .filter(|(unit, owner, _, mobile, attack)| {
            is_soldier(unit, **owner)
                && !mobile.is_moving()
                && attack.is_none_or(|a| a.target.is_none())
        })
        .count();

    let base_positions: Vec<Vec3> = buildings
        .iter()
        .filter(|(_, owner)| **owner == team)
        .map(|(building, _)| footprint_center(building.origin, building.kind.spec().footprint))
        .collect();
    let intruder = targets.iter().find(|t| {
        t.team.is_hostile_to(team)
            && base_positions
                .iter()
                .any(|base| base.distance(t.position) < BASE_DEFENSE_RADIUS)
    });

    if ai.assault_underway && soldiers == 0 {
        ai.assault_underway = false;
        ai.wave_size = (ai.wave_size + 2).min(MAX_WAVE_SIZE);
    }
    if !ai.assault_underway && idle_soldiers >= ai.wave_size {
        ai.assault_underway = true;
    }

    let home = cell_center(ai.base);
    let objective = intruder.map(|t| t.position).or_else(|| {
        ai.assault_underway
            .then(|| {
                targets
                    .iter()
                    .filter(|t| t.team.is_hostile_to(team))
                    .min_by(|a, b| {
                        let rank = |t: &&TargetInfo| {
                            let preference = if t.is_building { 0.0 } else { 30.0 };
                            t.position.distance(home) + preference
                        };
                        rank(a).total_cmp(&rank(b))
                    })
                    .map(|t| t.position)
            })
            .flatten()
    });
    let Some(objective) = objective else { return };
    let objective_cell = world_to_cell(objective);

    for (unit, &owner, transform, mut mobile, attack) in army.iter_mut() {
        let idle = !mobile.is_moving() && attack.is_none_or(|a| a.target.is_none());
        if is_soldier(unit, owner) && idle {
            order_move(
                &mut mobile,
                map,
                transform.translation,
                objective_cell,
                false,
            );
        }
    }
}
