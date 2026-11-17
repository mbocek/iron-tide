#![allow(clippy::type_complexity, clippy::too_many_arguments)]

pub mod ai;
pub mod ambience;
pub mod animation;
pub mod assets;
pub mod camera;
pub mod capture;
pub mod catalog;
pub mod combat;
pub mod components;
pub mod control_groups;
pub mod damage_feedback;
pub mod economy;
pub mod effects;
pub mod flat_scenery;
pub mod fog;
pub mod fog_view;
pub mod frame_pacing;
pub mod graphics;
pub mod headless;
pub mod intel;
pub mod levels;
pub mod main_menu;
pub mod map;
pub mod match_flow;
pub mod medical;
pub mod menu;
pub mod minimap;
pub mod movement;
pub mod notifications;
pub mod orders;
pub mod outcome;
pub mod overlay;
pub mod perf_overlay;
pub mod production;
pub mod queue_bar;
pub mod savegame;
pub mod scenery;
pub mod settings;
pub mod sound;
pub mod spawn;
pub mod structure_tools;
pub mod structures;
pub mod terrain;
pub mod terrain_material;
pub mod ui;
pub mod unit_orders;
pub mod veterancy;

use bevy::prelude::*;

/// Startup phases: assets must exist before the world is generated, and the interface reads the finished world.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum StartupPhase {
    Assets,
    World,
    Interface,
}

/// Per-frame phases: player input, then the battle simulation, then everything that only draws it.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum FramePhase {
    Input,
    Simulation,
    Presentation,
}

/// The battle itself: map, economy, production, enemy AI, movement, combat, saving and the outcome.
/// It needs `ModelLibrary` and `EffectAssets`; `GamePlugin` loads real ones, tests insert placeholders.
pub struct SimulationPlugin {
    pub enemy_ai: bool,
    pub starting_forces: bool,
}

impl Default for SimulationPlugin {
    fn default() -> Self {
        Self {
            enemy_ai: true,
            starting_forces: true,
        }
    }
}

impl Plugin for SimulationPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<bevy::state::app::StatesPlugin>() {
            app.add_plugins(bevy::state::app::StatesPlugin);
        }
        app.init_state::<match_flow::GameState>()
            .configure_sets(
                Startup,
                (
                    StartupPhase::Assets,
                    StartupPhase::World,
                    StartupPhase::Interface,
                )
                    .chain(),
            )
            .configure_sets(
                Update,
                (
                    FramePhase::Input,
                    FramePhase::Simulation,
                    FramePhase::Presentation,
                )
                    .chain()
                    .run_if(in_state(match_flow::GameState::Playing)),
            )
            .insert_resource(match_flow::MatchRules {
                enemy_ai: self.enemy_ai,
                starting_forces: self.starting_forces,
            })
            .init_resource::<levels::ActiveLevel>()
            .init_resource::<combat::Targets>()
            .init_resource::<economy::Economies>()
            .init_resource::<production::Production>()
            .init_resource::<production::Placement>()
            .init_resource::<ai::AiCommanders>()
            .init_resource::<outcome::GameOutcome>()
            .init_resource::<notifications::Notifications>()
            .init_resource::<control_groups::ControlGroups>()
            .init_resource::<match_flow::MatchClock>()
            .init_resource::<fog::FogOfWar>()
            .init_resource::<intel::Intel>()
            .init_resource::<veterancy::LastHits>()
            .init_resource::<savegame::SaveRequests>()
            .init_resource::<savegame::SaveDirectory>()
            .add_message::<combat::DamageDealt>()
            .add_message::<combat::Destroyed>()
            .add_message::<sound::SoundCue>()
            .add_message::<structures::StructureOrder>()
            .add_systems(
                OnEnter(match_flow::GameState::Playing),
                (
                    match_flow::unpause,
                    match_flow::reset_match_state,
                    map::generate_map,
                    match_flow::deploy_level_forces,
                    savegame::restore_pending_load,
                )
                    .chain(),
            )
            .add_systems(
                OnEnter(match_flow::GameState::Restarting),
                match_flow::continue_after_restart,
            )
            .add_systems(Update, savegame::begin_requested_load)
            .add_systems(
                Last,
                match_flow::scope_match_entities.run_if(in_state(match_flow::GameState::Playing)),
            )
            .add_systems(
                Update,
                (
                    (
                        match_flow::advance_match_clock,
                        economy::tally_power,
                        production::advance_production,
                        ai::command_enemies,
                        ai::repair_damaged_structures,
                        economy::run_harvesters,
                        structures::carry_out_structure_orders,
                        structures::repair_structures,
                        structures::finish_sales,
                        medical::treat_wounded,
                    )
                        .chain(),
                    (
                        fog::update_fog,
                        intel::update_intel,
                        combat::snapshot_targets,
                        combat::acquire_targets,
                        combat::chase_targets,
                        unit_orders::carry_out_orders,
                        movement::follow_paths,
                        movement::separate_units,
                        movement::settle_on_terrain,
                        combat::aim_and_fire,
                        combat::fly_projectiles,
                        combat::remove_destroyed,
                        economy::clear_depleted_crystals,
                        scenery::clear_decor_under_new_buildings,
                        effects::animate_blasts,
                    )
                        .chain(),
                    (
                        veterancy::record_last_hits,
                        veterancy::credit_kills,
                        veterancy::promote_veterans,
                        veterancy::elite_field_repairs,
                        notifications::watch_base,
                        outcome::decide_outcome,
                        savegame::write_requested_save,
                    )
                        .chain(),
                )
                    .chain()
                    .in_set(FramePhase::Simulation),
            );
    }
}

/// The full game: the simulation plus camera, interface, graphics, effects and animation.
pub struct GamePlugin;

impl Plugin for GamePlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(SimulationPlugin::default())
            .init_resource::<camera::Cursor>()
            .init_resource::<orders::DragSelection>()
            .init_resource::<orders::OrderMarker>()
            .init_resource::<ambience::CrystalCores>()
            .init_resource::<animation::AnimationLibrary>()
            .init_resource::<menu::MenuState>()
            .init_resource::<sound::MusicDirector>()
            .add_observer(sound::click_feedback)
            .add_systems(
                OnEnter(match_flow::GameState::Playing),
                sound::count_match_start,
            )
            .add_systems(
                Update,
                (
                    sound::play_sound_cues,
                    sound::direct_music,
                    sound::apply_master_volume,
                )
                    .after(FramePhase::Presentation),
            )
            .insert_resource(settings::load_settings())
            .init_gizmo_group::<overlay::HealthBarGizmos>()
            .add_systems(Last, frame_pacing::pace_frames)
            .add_systems(
                Update,
                (
                    flat_scenery::merge_scenery_models,
                    flat_scenery::dress_flat_scenery,
                )
                    .chain(),
            )
            .init_resource::<structure_tools::StructureTool>()
            .add_plugins((
                bevy::diagnostic::FrameTimeDiagnosticsPlugin::default(),
                bevy::diagnostic::SystemInformationDiagnosticsPlugin,
                bevy::diagnostic::EntityCountDiagnosticsPlugin::default(),
                bevy::render::diagnostic::RenderDiagnosticsPlugin,
                capture::CapturePlugin,
                MaterialPlugin::<terrain_material::TerrainMaterial>::default(),
                MaterialPlugin::<terrain_material::WaterMaterial>::default(),
            ))
            .add_systems(
                Startup,
                (
                    assets::load_models,
                    animation::load_animation_sources,
                    terrain_material::load_terrain_textures,
                    effects::create_effect_assets,
                    production::create_ghost_materials,
                    camera::setup_scene,
                    overlay::configure_gizmos,
                    ui::load_fonts,
                    fog_view::create_fog_texture,
                    sound::load_sounds,
                    flat_scenery::load_flat_scenery_sources,
                )
                    .chain()
                    .in_set(StartupPhase::Assets),
            )
            .add_systems(
                OnEnter(match_flow::GameState::Playing),
                (
                    camera::focus_on_player_base,
                    map::spawn_terrain_visuals,
                    fog_view::shroud_unexplored_scenery,
                    minimap::create_minimap,
                    ui::build_interface,
                    structure_tools::prepare_structure_tools,
                    orders::spawn_selection_box,
                    ambience::light_crystal_fields,
                    ambience::index_trampleable_vegetation,
                    menu::close_menu,
                )
                    .chain()
                    .after(match_flow::deploy_level_forces)
                    .before(savegame::restore_pending_load),
            )
            .add_systems(PostStartup, match_flow::finish_booting)
            .add_systems(
                OnEnter(match_flow::GameState::MainMenu),
                (match_flow::unpause, main_menu::build_main_menu),
            )
            .add_systems(
                Update,
                main_menu::refresh_main_menu.run_if(in_state(match_flow::GameState::MainMenu)),
            )
            .add_systems(
                Update,
                (
                    camera::track_cursor,
                    camera::control_camera,
                    control_groups::handle_group_keys,
                    graphics::cycle_quality_preset,
                    perf_overlay::toggle_performance_overlay,
                    menu::toggle_menu,
                    menu::pause_while_menu_open,
                    savegame::quick_save_keys,
                    menu::rebuild_menu_page,
                    menu::refresh_menu,
                    ui::toggle_controls_help,
                    orders::select_units,
                    orders::issue_orders,
                    production::place_structure,
                    structure_tools::apply_structure_tool,
                    structure_tools::switch_tools_by_keyboard,
                )
                    .chain()
                    .in_set(FramePhase::Input),
            )
            .add_systems(
                Update,
                (
                    (
                        economy::link_ore_load_nodes,
                        economy::show_harvester_load,
                        combat::link_turret_nodes,
                        combat::rotate_turret_nodes,
                        ambience::sway_vegetation,
                        ambience::trample_vegetation,
                        ambience::emit_crystal_sparkles,
                        ambience::discover_crystal_cores,
                        ambience::pulse_crystal_cores,
                        ambience::pulse_crystal_glow,
                        effects::animate_sparkles,
                        damage_feedback::spawn_damage_numbers,
                        damage_feedback::float_damage_numbers,
                        damage_feedback::emit_damage_smoke,
                        terrain_material::assemble_terrain_material,
                        terrain_material::animate_water,
                    )
                        .chain(),
                    (
                        graphics::apply_graphics_settings,
                        settings::apply_window_settings,
                        settings::apply_game_speed,
                        settings::persist_settings,
                        graphics::strip_fine_decor_shadows,
                        graphics::cull_distant_decor,
                        animation::build_animation_graphs,
                        animation::attach_animation_players,
                        animation::animate_soldiers,
                        animation::animate_vehicles,
                        animation::animate_crystals,
                        animation::animate_corpses,
                    )
                        .chain(),
                    (
                        overlay::draw_overlays,
                        overlay::draw_planned_routes,
                        structure_tools::draw_repair_markers,
                        structure_tools::refresh_tool_buttons,
                        structure_tools::refresh_tool_hint,
                        ui::refresh_build_tiles,
                        ui::refresh_resources,
                        ui::refresh_info_panel,
                        queue_bar::rebuild_queue_bar,
                        control_groups::refresh_group_bar,
                        perf_overlay::refresh_performance_overlay,
                        queue_bar::animate_queue_progress,
                        ui::refresh_notifications,
                        terrain_material::dress_terrain_surfaces,
                        fog_view::paint_fog_texture,
                        fog_view::reveal_explored_scenery,
                        fog_view::show_seen_enemies,
                        minimap::redraw_minimap,
                        ui::show_outcome,
                    )
                        .chain(),
                )
                    .chain()
                    .in_set(FramePhase::Presentation),
            );
    }
}
