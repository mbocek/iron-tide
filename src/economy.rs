use bevy::prelude::*;

use crate::catalog::{BuildingKind, HARVESTER_CAPACITY, STARTING_CREDITS, Team};
use crate::components::*;
use crate::determinism::{SimId, in_simulation_order};
use crate::map::{
    CrystalVisual, GameMap, cell_center, cells_by_distance, crystal_scale, map_cells, world_to_cell,
};
use crate::movement::order_move;
use crate::player_commands::LocalTeam;
use crate::sound::{Sound, SoundCue};
use crate::spawn::exit_cell;

const MINING_TICK_SECS: f32 = 0.5;
const CRYSTAL_PER_MINING_TICK: u32 = 25;
const UNLOAD_SECS: f32 = 2.0;
const FIELD_SEARCH_RADIUS: i32 = 40;
const ARRIVAL_DISTANCE: f32 = 0.6;
/// Production runs at this speed while a team consumes more power than it generates.
pub const LOW_POWER_PRODUCTION_RATE: f32 = 0.5;

#[derive(Clone, Copy, Default)]
pub struct TeamEconomy {
    pub credits: i32,
    pub power_supply: i32,
    pub power_demand: i32,
    /// Multiplier on delivered crystal; the difficulty setting of computer opponents.
    pub income_rate: f32,
    /// Credits earned from delivered crystal over the whole match.
    pub harvested: i32,
}

impl TeamEconomy {
    pub fn has_low_power(&self) -> bool {
        self.power_demand > self.power_supply
    }

    pub fn production_rate(&self) -> f32 {
        if self.has_low_power() {
            LOW_POWER_PRODUCTION_RATE
        } else {
            1.0
        }
    }
}

#[derive(Resource)]
pub struct Economies([TeamEconomy; 4]);

impl Default for Economies {
    fn default() -> Self {
        let start = TeamEconomy {
            credits: STARTING_CREDITS,
            income_rate: 1.0,
            ..default()
        };
        Self([start; 4])
    }
}

impl Economies {
    pub fn of(&self, team: Team) -> &TeamEconomy {
        &self.0[team.index()]
    }

    pub fn of_mut(&mut self, team: Team) -> &mut TeamEconomy {
        &mut self.0[team.index()]
    }
}

pub fn tally_power(mut economies: ResMut<Economies>, buildings: Query<(&Building, &Team)>) {
    for team in Team::ALL {
        let economy = economies.of_mut(team);
        economy.power_supply = 0;
        economy.power_demand = 0;
    }
    for (building, &team) in &buildings {
        let power = building.kind.spec().power;
        let economy = economies.of_mut(team);
        if power >= 0 {
            economy.power_supply += power;
        } else {
            economy.power_demand -= power;
        }
    }
}

pub fn run_harvesters(
    local: Res<LocalTeam>,
    mut cues: MessageWriter<SoundCue>,
    time: Res<Time>,
    mut map: ResMut<GameMap>,
    mut economies: ResMut<Economies>,
    mut harvesters: Query<(&Team, &Transform, &mut Harvester, &mut Mobile, &SimId)>,
    refineries: Query<(Entity, &Building, &Team, &Transform)>,
    mut crystal_visuals: Query<
        (&CrystalVisual, &mut Transform),
        (Without<Harvester>, Without<Building>),
    >,
) {
    let dt = time.delta_secs();
    let mut claimed: Vec<IVec2> = harvesters
        .iter()
        .filter_map(|(_, _, harvester, ..)| claimed_crystal(harvester.state))
        .collect();
    let in_order = in_simulation_order(harvesters.iter_mut(), |item| *item.4);
    for (&team, transform, mut harvester, mut mobile, _) in in_order {
        let position = transform.translation;
        let next_state = match harvester.state {
            HarvestState::Holding => None,
            HarvestState::SeekingField => {
                if harvester.cargo >= HARVESTER_CAPACITY {
                    head_to_refinery(&mut mobile, &map, position, team, &refineries)
                } else {
                    let search_from = harvester.preferred_field.unwrap_or(world_to_cell(position));
                    nearest_free_crystal(&map, search_from, &claimed)
                        .or_else(|| map.nearest_crystal(search_from, FIELD_SEARCH_RADIUS))
                        .or_else(|| distant_field(&map, search_from))
                        .filter(|&cell| order_move(&mut mobile, &map, position, cell, false))
                        .map(HarvestState::TravellingToField)
                }
            }
            HarvestState::TravellingToField(cell) => {
                if map.crystal_at(cell) == 0 {
                    Some(HarvestState::SeekingField)
                } else if !mobile.is_moving() {
                    let arrived =
                        position.xz().distance(cell_center(cell).xz()) < ARRIVAL_DISTANCE * 2.0;
                    Some(if arrived {
                        HarvestState::Mining {
                            cell,
                            timer: MINING_TICK_SECS,
                        }
                    } else {
                        HarvestState::SeekingField
                    })
                } else {
                    None
                }
            }
            HarvestState::Mining { cell, timer } => {
                let timer = timer - dt;
                if timer > 0.0 {
                    Some(HarvestState::Mining { cell, timer })
                } else {
                    let room = HARVESTER_CAPACITY - harvester.cargo;
                    let (mined, depleted) =
                        map.mine_crystal(cell, room.min(CRYSTAL_PER_MINING_TICK));
                    harvester.cargo += mined;
                    harvester.preferred_field = Some(cell);
                    if !depleted {
                        resize_crystal_visual(&map, &mut crystal_visuals, cell);
                    }
                    if harvester.cargo >= HARVESTER_CAPACITY {
                        head_to_refinery(&mut mobile, &map, position, team, &refineries)
                    } else if depleted {
                        Some(HarvestState::SeekingField)
                    } else {
                        Some(HarvestState::Mining {
                            cell,
                            timer: MINING_TICK_SECS,
                        })
                    }
                }
            }
            HarvestState::ReturningToRefinery(refinery) => match refineries.get(refinery) {
                Err(_) => Some(HarvestState::SeekingField),
                Ok(_) if mobile.is_moving() => None,
                Ok((_, building, _, _)) => {
                    let dock = cell_center(exit_cell(building));
                    if position.xz().distance(dock.xz()) < ARRIVAL_DISTANCE * 2.0 {
                        Some(HarvestState::Unloading {
                            refinery,
                            timer: UNLOAD_SECS,
                        })
                    } else {
                        head_to_refinery(&mut mobile, &map, position, team, &refineries)
                    }
                }
            },
            HarvestState::Unloading { refinery, timer } => {
                let timer = timer - dt;
                if refineries.get(refinery).is_err() {
                    Some(HarvestState::SeekingField)
                } else if timer > 0.0 {
                    Some(HarvestState::Unloading { refinery, timer })
                } else {
                    let economy = economies.of_mut(team);
                    let earned = (harvester.cargo as f32 * economy.income_rate).round() as i32;
                    economy.credits += earned;
                    economy.harvested += earned;
                    harvester.cargo = 0;
                    if team == local.0 {
                        cues.write(SoundCue::at(Sound::Credits, position));
                    }
                    Some(HarvestState::SeekingField)
                }
            }
        };
        if let Some(state) = next_state {
            claimed.extend(claimed_crystal(state));
            harvester.state = state;
        }
    }
}

fn claimed_crystal(state: HarvestState) -> Option<IVec2> {
    match state {
        HarvestState::TravellingToField(cell) | HarvestState::Mining { cell, .. } => Some(cell),
        _ => None,
    }
}

/// Once the fields within reach are mined out, a harvester drives to the nearest crystal anywhere on the map
/// rather than idling beside its refinery.
fn distant_field(map: &GameMap, from: IVec2) -> Option<IVec2> {
    map.has_crystal_left()
        .then(|| map.nearest_crystal(from, map_cells()))
        .flatten()
}

/// Harvesters spread over a field instead of crowding onto one cell and shoving each other off it.
fn nearest_free_crystal(map: &GameMap, from: IVec2, claimed: &[IVec2]) -> Option<IVec2> {
    cells_by_distance(from, FIELD_SEARCH_RADIUS)
        .find(|cell| map.crystal_at(*cell) > 0 && !claimed.contains(cell))
}

fn head_to_refinery(
    mobile: &mut Mobile,
    map: &GameMap,
    position: Vec3,
    team: Team,
    refineries: &Query<(Entity, &Building, &Team, &Transform)>,
) -> Option<HarvestState> {
    let (refinery, building, _, _) = refineries
        .iter()
        .filter(|(_, building, owner, _)| {
            **owner == team && building.kind == BuildingKind::Refinery
        })
        .min_by(|a, b| {
            let distance = |t: &Transform| t.translation.distance_squared(position);
            let site = |building: &Building| (building.origin.y, building.origin.x);
            distance(a.3)
                .total_cmp(&distance(b.3))
                .then(site(a.1).cmp(&site(b.1)))
        })?;
    order_move(mobile, map, position, exit_cell(building), false)
        .then_some(HarvestState::ReturningToRefinery(refinery))
}

fn resize_crystal_visual(
    map: &GameMap,
    visuals: &mut Query<(&CrystalVisual, &mut Transform), (Without<Harvester>, Without<Building>)>,
    cell: IVec2,
) {
    if let Some((visual, mut transform)) =
        visuals.iter_mut().find(|(visual, _)| visual.cell == cell)
    {
        transform.scale = Vec3::splat(visual.size * crystal_scale(map.crystal_at(cell)));
    }
}

pub fn clear_depleted_crystals(
    mut commands: Commands,
    map: Res<GameMap>,
    visuals: Query<(Entity, &CrystalVisual)>,
) {
    if !map.is_changed() {
        return;
    }
    for (entity, visual) in &visuals {
        if map.crystal_at(visual.cell) == 0 {
            commands.entity(entity).despawn();
        }
    }
}

const EMPTY_LOAD_SCALE: f32 = 0.05;
const LOAD_NODE_NAME: &str = "OreLoad";

/// The hopper's ore heap inside the harvester model, scaled up from its floor as cargo accumulates.
#[derive(Component)]
pub struct OreLoadLink {
    node: Entity,
    full_scale: Vec3,
}

pub fn link_ore_load_nodes(
    mut commands: Commands,
    unlinked: Query<Entity, (With<Harvester>, Without<OreLoadLink>)>,
    children: Query<&Children>,
    nodes: Query<(&Name, &Transform)>,
) {
    for harvester in &unlinked {
        let found = children.iter_descendants(harvester).find_map(|node| {
            let (name, transform) = nodes.get(node).ok()?;
            (name.as_str() == LOAD_NODE_NAME).then_some((node, transform.scale))
        });
        if let Some((node, full_scale)) = found {
            commands
                .entity(harvester)
                .insert(OreLoadLink { node, full_scale });
        }
    }
}

pub fn show_harvester_load(
    harvesters: Query<(&Harvester, &OreLoadLink)>,
    mut nodes: Query<&mut Transform, Without<Harvester>>,
) {
    for (harvester, link) in &harvesters {
        let Ok(mut transform) = nodes.get_mut(link.node) else {
            continue;
        };
        let fill = (harvester.cargo as f32 / HARVESTER_CAPACITY as f32).clamp(0.0, 1.0);
        let height = EMPTY_LOAD_SCALE + (1.0 - EMPTY_LOAD_SCALE) * fill;
        let breadth = 0.8 + 0.2 * fill;
        transform.scale = link.full_scale * Vec3::new(breadth, height, breadth);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn production_slows_only_when_demand_exceeds_supply() {
        let balanced = TeamEconomy {
            credits: 0,
            power_supply: 100,
            power_demand: 100,
            ..default()
        };
        let starved = TeamEconomy {
            credits: 0,
            power_supply: 100,
            power_demand: 101,
            ..default()
        };
        assert_eq!(balanced.production_rate(), 1.0);
        assert_eq!(starved.production_rate(), LOW_POWER_PRODUCTION_RATE);
    }
}
