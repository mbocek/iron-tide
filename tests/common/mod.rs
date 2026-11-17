#![allow(dead_code)]

use std::path::PathBuf;
use std::time::Duration;

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use iron_tide::SimulationPlugin;
use iron_tide::assets::ModelLibrary;
use iron_tide::catalog::{BuildingKind, Team, UnitKind};
use iron_tide::combat::DamageDealt;
use iron_tide::components::*;
use iron_tide::economy::Economies;
use iron_tide::effects::EffectAssets;
use iron_tide::levels::{ActiveLevel, LevelDefinition};
use iron_tide::map::{GameMap, cell_center};
use iron_tide::match_flow::GameState;
use iron_tide::movement::order_move;
use iron_tide::outcome::GameOutcome;
use iron_tide::production::{BuildItem, Production, construct_building};
use iron_tide::savegame::SaveDirectory;
use iron_tide::spawn::spawn_unit;

/// Simulation step; small enough that the fastest projectiles and units behave as in the real game.
pub const STEP: Duration = Duration::from_millis(50);

/// Damage dealt to each team since the battle started, collected from `DamageDealt` messages.
#[derive(Resource, Default)]
pub struct DamageLedger {
    pub taken: [f32; 4],
}

fn record_damage(mut hits: MessageReader<DamageDealt>, mut ledger: ResMut<DamageLedger>) {
    for hit in hits.read() {
        ledger.taken[hit.team.index()] += hit.amount;
    }
}

/// A headless match running the real simulation systems on a fixed clock.
pub struct Battle {
    pub app: App,
    pub elapsed_secs: f32,
    save_directory: PathBuf,
}

pub struct BattleSetup {
    pub enemy_ai: bool,
    pub starting_forces: bool,
    /// The default skirmish (one Normal opponent) when `None`.
    pub level: Option<LevelDefinition>,
}

impl Battle {
    pub fn new(setup: BattleSetup) -> Self {
        let save_directory = std::env::temp_dir().join(format!(
            "iron_tide_test_{}_{}",
            std::process::id(),
            rand_suffix()
        ));
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(TimeUpdateStrategy::ManualDuration(STEP))
            .insert_resource(ModelLibrary::placeholder())
            .insert_resource(EffectAssets::placeholder())
            .insert_resource(SaveDirectory(save_directory.clone()))
            .init_resource::<DamageLedger>()
            .add_plugins(SimulationPlugin {
                enemy_ai: setup.enemy_ai,
                starting_forces: setup.starting_forces,
            })
            .add_systems(
                Update,
                record_damage.after(iron_tide::FramePhase::Simulation),
            );
        if let Some(level) = setup.level {
            app.insert_resource(ActiveLevel(level));
        }
        let startup_frame_limit = 10;
        for _ in 0..startup_frame_limit {
            let state = *app.world().resource::<State<GameState>>().get();
            if state == GameState::Playing && app.world().contains_resource::<GameMap>() {
                break;
            }
            app.world_mut()
                .resource_mut::<NextState<GameState>>()
                .set(GameState::Playing);
            app.update();
        }
        assert!(
            app.world().contains_resource::<GameMap>(),
            "the match never started"
        );
        Self {
            app,
            elapsed_secs: 0.0,
            save_directory,
        }
    }

    pub fn standard() -> Self {
        Self::new(BattleSetup {
            enemy_ai: true,
            starting_forces: true,
            level: None,
        })
    }

    pub fn sandbox() -> Self {
        Self::new(BattleSetup {
            enemy_ai: false,
            starting_forces: false,
            level: None,
        })
    }

    pub fn world(&mut self) -> &mut World {
        self.app.world_mut()
    }

    pub fn run_for(&mut self, secs: f32) {
        let steps = (secs / STEP.as_secs_f32()).ceil() as usize;
        for _ in 0..steps {
            self.app.update();
            self.elapsed_secs += STEP.as_secs_f32();
        }
    }

    /// Runs until `done` holds or `limit_secs` of game time pass; returns the game time it took.
    pub fn run_until(
        &mut self,
        limit_secs: f32,
        mut done: impl FnMut(&mut Battle) -> bool,
    ) -> Option<f32> {
        let start = self.elapsed_secs;
        let check_every_secs = 1.0;
        while self.elapsed_secs - start < limit_secs {
            self.run_for(check_every_secs);
            if done(self) {
                return Some(self.elapsed_secs - start);
            }
        }
        None
    }

    pub fn spawn_unit(&mut self, kind: UnitKind, team: Team, cell: IVec2) -> Entity {
        let world = self.world();
        let entity = world.resource_scope(|world, models: Mut<ModelLibrary>| {
            let mut commands = world.commands();
            spawn_unit(&mut commands, &models, kind, team, cell_center(cell), 0.0)
        });
        world.flush();
        entity
    }

    pub fn construct(&mut self, kind: BuildingKind, team: Team, origin: IVec2) {
        let world = self.world();
        world.resource_scope(|world, models: Mut<ModelLibrary>| {
            world.resource_scope(|world, mut map: Mut<GameMap>| {
                let mut commands = world.commands();
                construct_building(&mut commands, &models, &mut map, kind, team, origin);
            });
        });
        world.flush();
    }

    pub fn order_move(&mut self, unit: Entity, goal: IVec2) {
        let world = self.world();
        world.resource_scope(|world, map: Mut<GameMap>| {
            let position = world
                .get::<Transform>(unit)
                .expect("unit exists")
                .translation;
            let mut mobile = world.get_mut::<Mobile>(unit).expect("unit is mobile");
            assert!(
                order_move(&mut mobile, &map, position, goal, true),
                "no path to {goal}"
            );
        });
    }

    pub fn enqueue(&mut self, team: Team, item: BuildItem) -> bool {
        let world = self.world();
        let owned: Vec<BuildingKind> = world
            .query::<(&Building, &Team)>()
            .iter(world)
            .filter(|(_, owner)| **owner == team)
            .map(|(building, _)| building.kind)
            .collect();
        world.resource_scope(|world, mut production: Mut<Production>| {
            let mut economies = world.resource_mut::<Economies>();
            production
                .enqueue(&mut economies, team, item, &owned)
                .is_ok()
        })
    }

    pub fn units(&mut self, team: Team, kind: Option<UnitKind>) -> Vec<Entity> {
        let world = self.world();
        world
            .query::<(Entity, &Unit, &Team)>()
            .iter(world)
            .filter(|(_, unit, owner)| **owner == team && kind.is_none_or(|k| unit.0 == k))
            .map(|(entity, ..)| entity)
            .collect()
    }

    pub fn buildings(&mut self, team: Team) -> Vec<BuildingKind> {
        let world = self.world();
        world
            .query::<(&Building, &Team)>()
            .iter(world)
            .filter(|(_, owner)| **owner == team)
            .map(|(building, _)| building.kind)
            .collect()
    }

    pub fn alive(&mut self, entity: Entity) -> bool {
        self.world().get_entity(entity).is_ok()
    }

    pub fn position(&mut self, entity: Entity) -> Vec3 {
        self.world()
            .get::<Transform>(entity)
            .expect("entity exists")
            .translation
    }

    pub fn credits(&mut self, team: Team) -> i32 {
        self.world().resource::<Economies>().of(team).credits
    }

    pub fn set_credits(&mut self, team: Team, credits: i32) {
        self.world()
            .resource_mut::<Economies>()
            .of_mut(team)
            .credits = credits;
    }

    pub fn outcome(&mut self) -> GameOutcome {
        *self.world().resource::<GameOutcome>()
    }

    pub fn damage_taken(&mut self, team: Team) -> f32 {
        self.world().resource::<DamageLedger>().taken[team.index()]
    }

    pub fn despawn_units(&mut self, team: Team) {
        for unit in self.units(team, None) {
            self.world().despawn(unit);
        }
    }
}

impl Drop for Battle {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.save_directory);
    }
}

fn rand_suffix() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

impl Battle {
    pub fn on_level(level: LevelDefinition) -> Self {
        Self::new(BattleSetup {
            enemy_ai: true,
            starting_forces: true,
            level: Some(level),
        })
    }

    pub fn state(&mut self) -> GameState {
        *self.world().resource::<State<GameState>>().get()
    }

    pub fn hostile_units(&mut self, team: Team) -> usize {
        let world = self.world();
        world
            .query::<&Team>()
            .iter(world)
            .filter(|owner| owner.is_hostile_to(team))
            .count()
    }
}
