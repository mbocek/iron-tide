use bevy::prelude::*;

use crate::ai::{AiCommander, AiCommanders, find_building_site};
use crate::assets::ModelLibrary;
use crate::catalog::{BuildingKind, Team, UnitKind};
use crate::combat::Targets;
use crate::control_groups::ControlGroups;
use crate::economy::Economies;
use crate::fog::FogOfWar;
use crate::levels::{ActiveLevel, ForceSetup};
use crate::map::{GameMap, cell_center};
use crate::notifications::Notifications;
use crate::outcome::GameOutcome;
use crate::production::{BuildItem, Placement, PlacementContext, Production, construct_building};
use crate::savegame::PendingLoad;
use crate::spawn::{spawn_building, spawn_unit};

#[derive(States, Default, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GameState {
    /// Bevy enters the initial state before `Startup`, so the menu waits here until assets are requested.
    #[default]
    Booting,
    MainMenu,
    Playing,
    /// Passes straight back to `Playing`; leaving `Playing` clears the old match first.
    Restarting,
}

/// Entities that outlive matches: the camera and the sun.
#[derive(Component, Clone)]
pub struct Persistent;

/// How a match is assembled; tests switch parts off to build isolated scenarios.
#[derive(Resource, Clone, Copy)]
pub struct MatchRules {
    pub enemy_ai: bool,
    pub starting_forces: bool,
}

/// Game time elapsed in the current match; pauses with the game.
#[derive(Resource, Default)]
pub struct MatchClock {
    pub elapsed_secs: f32,
}

pub fn reset_match_state(mut commands: Commands, level: Res<ActiveLevel>) {
    commands.insert_resource(FogOfWar::new(level.0.fog_of_war));
    commands.insert_resource(Targets::default());
    commands.insert_resource(Economies::default());
    commands.insert_resource(Production::default());
    commands.insert_resource(Placement::default());
    commands.insert_resource(AiCommanders::default());
    commands.insert_resource(GameOutcome::default());
    commands.insert_resource(Notifications::default());
    commands.insert_resource(ControlGroups::default());
    commands.insert_resource(MatchClock::default());
}

pub fn unpause(mut time: ResMut<Time<Virtual>>) {
    time.unpause();
}

pub fn advance_match_clock(time: Res<Time>, mut clock: ResMut<MatchClock>) {
    clock.elapsed_secs += time.delta_secs();
}

pub fn finish_booting(state: Res<State<GameState>>, mut next: ResMut<NextState<GameState>>) {
    if *state.get() == GameState::Booting {
        next.set(GameState::MainMenu);
    }
}

pub fn continue_after_restart(mut next: ResMut<NextState<GameState>>) {
    next.set(GameState::Playing);
}

/// Everything spawned while a match runs goes away when it ends, except entities marked `Persistent`.
pub fn scope_match_entities(
    mut commands: Commands,
    unscoped: Query<
        Entity,
        (
            Or<(With<Transform>, With<Node>)>,
            Without<ChildOf>,
            Without<Persistent>,
            Without<DespawnOnExit<GameState>>,
        ),
    >,
) {
    for entity in &unscoped {
        commands
            .entity(entity)
            .insert(DespawnOnExit(GameState::Playing));
    }
}

fn forbidden_items(level: &crate::levels::LevelDefinition) -> Vec<BuildItem> {
    let units = UnitKind::ALL
        .into_iter()
        .filter(|&k| !level.allows_unit(k))
        .map(BuildItem::Unit);
    let buildings = BuildingKind::CONSTRUCTIBLE
        .into_iter()
        .filter(|&k| !level.allows_building(k))
        .map(BuildItem::Structure);
    units.chain(buildings).collect()
}

fn deploy_force(
    commands: &mut Commands,
    models: &ModelLibrary,
    map: &mut GameMap,
    team: Team,
    force: &ForceSetup,
) {
    let base = force.site.origin();
    spawn_building(
        commands,
        models,
        map,
        BuildingKind::CommandCenter,
        team,
        base,
    );
    let mut own_buildings = vec![(base, BuildingKind::CommandCenter.spec().footprint)];
    for &kind in &force.buildings {
        let context = PlacementContext {
            map,
            own_buildings: own_buildings.clone(),
            occupied_cells: Vec::new(),
        };
        let Some(origin) = find_building_site(&context, base, kind) else {
            warn!("no room for a starting {kind:?} of {team:?}");
            continue;
        };
        construct_building(commands, models, map, kind, team, origin);
        own_buildings.push((origin, kind.spec().footprint));
    }

    let rally = base + force.site.rally_offset();
    let columns = 3;
    for (i, &kind) in force.units.iter().enumerate() {
        let wanted = rally + IVec2::new(i as i32 % columns, i as i32 / columns);
        let cell = map.nearest_passable(wanted).unwrap_or(wanted);
        spawn_unit(
            commands,
            models,
            kind,
            team,
            cell_center(cell),
            force.site.facing(),
        );
    }
}

/// Builds the opening position of the active level: bases, armies, money, opponents and tech limits.
pub fn deploy_level_forces(
    mut commands: Commands,
    models: Res<ModelLibrary>,
    level: Res<ActiveLevel>,
    rules: Res<MatchRules>,
    pending_load: Option<Res<PendingLoad>>,
    mut map: ResMut<GameMap>,
    mut economies: ResMut<Economies>,
    mut production: ResMut<Production>,
    mut commanders: ResMut<AiCommanders>,
) {
    let level = &level.0;
    let forbidden = forbidden_items(level);
    for team in Team::ALL {
        production.forbid(team, forbidden.clone());
    }
    if pending_load.is_some() {
        return;
    }

    economies.of_mut(Team::Player).credits = level.player.credits;
    for (team, enemy) in level.opponents() {
        let economy = economies.of_mut(team);
        economy.credits = enemy.force.credits;
        economy.income_rate = enemy.difficulty.income_rate();
        if rules.enemy_ai {
            commanders.0.push(AiCommander::new(
                team,
                enemy.force.site.origin(),
                enemy.difficulty,
            ));
        }
    }
    if !rules.starting_forces {
        return;
    }
    deploy_force(
        &mut commands,
        &models,
        &mut map,
        Team::Player,
        &level.player,
    );
    for (team, enemy) in level.opponents() {
        deploy_force(&mut commands, &models, &mut map, team, &enemy.force);
    }
}
