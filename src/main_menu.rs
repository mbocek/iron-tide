use bevy::picking::hover::Hovered;
use bevy::prelude::*;

use crate::catalog::Team;
use crate::levels::{
    ActiveLevel, BaseSite, Difficulty, LevelDefinition, LevelEntry, LevelSource, ObjectiveKind,
    SkirmishOptions, TechLevel, campaign, custom_levels, custom_levels_directory,
    save_custom_level,
};
use crate::match_flow::GameState;
use crate::savegame::{QUICK_SAVE_SLOT, SaveDirectory, SaveRequests, slot_summary};
use crate::ui::{ACCENT, FRAME, FRAME_BRIGHT, GOOD, PANEL, TEXT_DIM, TEXT_MAIN, UiFonts};

const LIST_WIDTH: f32 = 400.0;
const DETAIL_WIDTH: f32 = 620.0;
const BUTTON_IDLE: Color = Color::srgb(0.13, 0.15, 0.17);
const BUTTON_HOVERED: Color = Color::srgb(0.19, 0.22, 0.25);
const BUTTON_SELECTED: Color = Color::srgb(0.17, 0.24, 0.32);
const BACKDROP: Color = Color::srgb(0.035, 0.045, 0.055);
const CREDIT_CHOICES: [i32; 4] = [3000, 5000, 8000, 12000];
const MAP_VARIANTS: u64 = 5;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Selection {
    Level(usize),
    Editor,
}

#[derive(Resource)]
pub struct MainMenu {
    entries: Vec<LevelEntry>,
    selection: Selection,
    options: SkirmishOptions,
    message: Option<String>,
    needs_rebuild: bool,
}

#[derive(Component)]
pub struct MainMenuRoot;

#[derive(Component)]
pub struct LevelList;

#[derive(Component)]
pub struct DetailPanel;

#[derive(Component)]
pub struct ListButton(Selection);

#[derive(Component)]
pub struct EditorOption(EditorField);

#[derive(Component, Clone, Copy)]
pub enum MenuCommand {
    StartSelected,
    StartEditorBattle,
    SaveEditorLevel,
    ContinueQuickSave,
    Quit,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EditorField {
    Enemies,
    Difficulty,
    Credits,
    Objective,
    Tech,
    MapVariant,
    PlayerSite,
    FogOfWar,
}

const EDITOR_FIELDS: [EditorField; 8] = [
    EditorField::Enemies,
    EditorField::Difficulty,
    EditorField::Credits,
    EditorField::Objective,
    EditorField::Tech,
    EditorField::MapVariant,
    EditorField::PlayerSite,
    EditorField::FogOfWar,
];

fn cycle<T: Copy + PartialEq>(choices: &[T], current: T, forward: bool) -> T {
    let index = choices.iter().position(|&c| c == current).unwrap_or(0);
    let next = if forward {
        (index + 1) % choices.len()
    } else {
        (index + choices.len() - 1) % choices.len()
    };
    choices[next]
}

impl EditorField {
    fn label(self) -> &'static str {
        match self {
            EditorField::Enemies => "Enemy commanders",
            EditorField::Difficulty => "Difficulty",
            EditorField::Credits => "Starting credits",
            EditorField::Objective => "Objective",
            EditorField::Tech => "Available units",
            EditorField::MapVariant => "Map variant",
            EditorField::PlayerSite => "Your base",
            EditorField::FogOfWar => "Fog of war",
        }
    }

    fn value(self, options: &SkirmishOptions) -> String {
        match self {
            EditorField::Enemies => options.enemies.to_string(),
            EditorField::Difficulty => format!("{:?}", options.difficulty),
            EditorField::Credits => format!("${}", options.credits),
            EditorField::Objective => match options.objective {
                ObjectiveKind::Annihilate => "Destroy everything".into(),
                ObjectiveKind::Decapitate => "Destroy Command Centers".into(),
                ObjectiveKind::Survive => "Survive 10 minutes".into(),
                ObjectiveKind::Economy => "Stockpile credits".into(),
            },
            EditorField::Tech => options.tech.label().into(),
            EditorField::MapVariant => format!("{} of {MAP_VARIANTS}", options.map_variant),
            EditorField::PlayerSite => site_name(options.player_site).into(),
            EditorField::FogOfWar => if options.fog_of_war { "On" } else { "Off" }.into(),
        }
    }

    fn change(self, options: &mut SkirmishOptions, forward: bool) {
        match self {
            EditorField::Enemies => options.enemies = cycle(&[1, 2, 3], options.enemies, forward),
            EditorField::Difficulty => {
                options.difficulty = cycle(
                    &[Difficulty::Easy, Difficulty::Normal, Difficulty::Hard],
                    options.difficulty,
                    forward,
                );
            }
            EditorField::Credits => {
                options.credits = cycle(&CREDIT_CHOICES, options.credits, forward)
            }
            EditorField::Objective => {
                let kinds = [
                    ObjectiveKind::Annihilate,
                    ObjectiveKind::Decapitate,
                    ObjectiveKind::Survive,
                    ObjectiveKind::Economy,
                ];
                options.objective = cycle(&kinds, options.objective, forward);
            }
            EditorField::Tech => {
                options.tech = cycle(
                    &[TechLevel::Basic, TechLevel::Standard, TechLevel::Full],
                    options.tech,
                    forward,
                );
            }
            EditorField::MapVariant => {
                let variants: Vec<u64> = (1..=MAP_VARIANTS).collect();
                options.map_variant = cycle(&variants, options.map_variant, forward);
            }
            EditorField::PlayerSite => {
                options.player_site = cycle(&BaseSite::ALL, options.player_site, forward)
            }
            EditorField::FogOfWar => options.fog_of_war = !options.fog_of_war,
        }
    }
}

fn site_name(site: BaseSite) -> &'static str {
    match site {
        BaseSite::SouthWest => "South-west",
        BaseSite::NorthEast => "North-east",
        BaseSite::NorthWest => "North-west",
        BaseSite::SouthEast => "South-east",
    }
}

fn all_levels() -> Vec<LevelEntry> {
    let mut entries = campaign();
    entries.extend(custom_levels(&custom_levels_directory()));
    entries
}

pub fn build_main_menu(
    mut commands: Commands,
    fonts: Res<UiFonts>,
    save_directory: Res<SaveDirectory>,
) {
    let menu = MainMenu {
        entries: all_levels(),
        selection: Selection::Level(0),
        options: SkirmishOptions::default(),
        message: None,
        needs_rebuild: true,
    };
    let has_quick_save = slot_summary(&save_directory, QUICK_SAVE_SLOT);
    commands
        .spawn((
            MainMenuRoot,
            DespawnOnExit(GameState::MainMenu),
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                right: Val::Px(0.0),
                top: Val::Px(0.0),
                bottom: Val::Px(0.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                column_gap: Val::Px(18.0),
                ..default()
            },
            BackgroundColor(BACKDROP),
        ))
        .with_children(|root| {
            root.spawn((
                Node {
                    width: Val::Px(LIST_WIDTH),
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::all(Val::Px(18.0)),
                    row_gap: Val::Px(8.0),
                    border: UiRect::all(Val::Px(2.0)),
                    ..default()
                },
                BackgroundColor(PANEL),
                BorderColor::all(FRAME_BRIGHT),
            ))
            .with_children(|column| {
                column.spawn(fonts.heading("IRON TIDE", 46.0, ACCENT));
                column.spawn(fonts.text("Coalition command", 18.0, Team::Player.ui_color()));
                if let Some(summary) = has_quick_save {
                    spawn_command_button(
                        column,
                        &fonts,
                        &format!("Continue  ({summary})"),
                        MenuCommand::ContinueQuickSave,
                    );
                }
                column.spawn((
                    LevelList,
                    Node {
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(6.0),
                        ..default()
                    },
                ));
                spawn_command_button(column, &fonts, "Quit to desktop", MenuCommand::Quit);
            });
            root.spawn((
                DetailPanel,
                Node {
                    width: Val::Px(DETAIL_WIDTH),
                    min_height: Val::Px(560.0),
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::all(Val::Px(18.0)),
                    row_gap: Val::Px(8.0),
                    border: UiRect::all(Val::Px(2.0)),
                    ..default()
                },
                BackgroundColor(PANEL),
                BorderColor::all(FRAME),
            ));
        });
    commands.insert_resource(menu);
}

fn button_node(width: Val) -> Node {
    Node {
        width,
        min_height: Val::Px(32.0),
        padding: UiRect::axes(Val::Px(10.0), Val::Px(4.0)),
        align_items: AlignItems::Center,
        border: UiRect::all(Val::Px(1.0)),
        ..default()
    }
}

fn spawn_command_button(
    parent: &mut ChildSpawnerCommands,
    fonts: &UiFonts,
    label: &str,
    command: MenuCommand,
) {
    parent
        .spawn((
            command,
            Hovered::default(),
            button_node(Val::Percent(100.0)),
            BackgroundColor(BUTTON_IDLE),
            BorderColor::all(FRAME),
        ))
        .with_children(|button| {
            button.spawn(fonts.heading(label, 17.0, TEXT_MAIN));
        })
        .observe(run_command);
}

fn spawn_list_button(
    parent: &mut ChildSpawnerCommands,
    fonts: &UiFonts,
    label: &str,
    selection: Selection,
) {
    parent
        .spawn((
            ListButton(selection),
            Hovered::default(),
            button_node(Val::Percent(100.0)),
            BackgroundColor(BUTTON_IDLE),
            BorderColor::all(FRAME),
        ))
        .with_children(|button| {
            button.spawn(fonts.heading(label, 17.0, TEXT_MAIN));
        })
        .observe(select_entry);
}

fn rebuild_level_list(commands: &mut Commands, list: Entity, fonts: &UiFonts, menu: &MainMenu) {
    commands.entity(list).despawn_related::<Children>();
    commands.entity(list).with_children(|list| {
        list.spawn(fonts.heading("CAMPAIGN", 15.0, TEXT_DIM));
        for (index, entry) in menu.entries.iter().enumerate() {
            if index > 0
                && matches!(entry.source, LevelSource::Custom(_))
                && !matches!(menu.entries[index - 1].source, LevelSource::Custom(_))
            {
                list.spawn(fonts.heading("CUSTOM LEVELS", 15.0, TEXT_DIM));
            }
            spawn_list_button(list, fonts, &entry.level.name, Selection::Level(index));
        }
        if !menu
            .entries
            .iter()
            .any(|entry| matches!(entry.source, LevelSource::Custom(_)))
        {
            list.spawn(fonts.text(
                "No custom levels yet - make one in the battle editor.",
                14.0,
                TEXT_DIM,
            ));
        }
        list.spawn(fonts.heading("QUICK BATTLE", 15.0, TEXT_DIM));
        spawn_list_button(list, fonts, "Battle editor", Selection::Editor);
    });
}

fn describe_level(level: &LevelDefinition) -> Vec<(String, Color)> {
    let mut lines = vec![
        (level.briefing.clone(), TEXT_MAIN),
        (String::new(), TEXT_DIM),
    ];
    lines.push((format!("Objective: {}", level.objective.describe()), ACCENT));
    lines.push((
        format!(
            "Your base: {}  -  ${}",
            site_name(level.player.site),
            level.player.credits
        ),
        Team::Player.ui_color(),
    ));
    for (team, enemy) in level.opponents() {
        lines.push((
            format!(
                "{}: {}, {:?}, ${}",
                team.name(),
                site_name(enemy.force.site),
                enemy.difficulty,
                enemy.force.credits
            ),
            team.ui_color(),
        ));
    }
    let units = if level.available_units.is_empty() {
        "every unit".to_string()
    } else {
        level
            .available_units
            .iter()
            .map(|kind| kind.spec().name)
            .collect::<Vec<_>>()
            .join(", ")
    };
    lines.push((format!("Available: {units}"), TEXT_DIM));
    lines
}

fn rebuild_details(commands: &mut Commands, panel: Entity, fonts: &UiFonts, menu: &MainMenu) {
    commands.entity(panel).despawn_related::<Children>();
    commands
        .entity(panel)
        .with_children(|panel| match menu.selection {
            Selection::Level(index) => {
                let level = &menu.entries[index].level;
                panel.spawn(fonts.heading(&level.name, 30.0, ACCENT));
                for (line, color) in describe_level(level) {
                    panel.spawn(fonts.text(line, 17.0, color));
                }
                spawn_command_button(panel, fonts, "START MISSION", MenuCommand::StartSelected);
            }
            Selection::Editor => {
                panel.spawn(fonts.heading("Battle editor", 30.0, ACCENT));
                panel.spawn(fonts.text(
                    "Left click: next value, right click: previous.",
                    15.0,
                    TEXT_DIM,
                ));
                for field in EDITOR_FIELDS {
                    panel
                        .spawn(Node {
                            justify_content: JustifyContent::SpaceBetween,
                            align_items: AlignItems::Center,
                            ..default()
                        })
                        .with_children(|row| {
                            row.spawn(fonts.text(field.label(), 18.0, TEXT_MAIN));
                            row.spawn((
                                EditorOption(field),
                                Hovered::default(),
                                button_node(Val::Px(260.0)),
                                BackgroundColor(BUTTON_IDLE),
                                BorderColor::all(FRAME),
                            ))
                            .with_children(|button| {
                                button.spawn(fonts.heading(
                                    field.value(&menu.options),
                                    16.0,
                                    ACCENT,
                                ));
                            })
                            .observe(change_option);
                        });
                }
                panel.spawn(fonts.text(
                    menu.options.to_level().objective.describe(),
                    16.0,
                    TEXT_DIM,
                ));
                spawn_command_button(panel, fonts, "START BATTLE", MenuCommand::StartEditorBattle);
                spawn_command_button(
                    panel,
                    fonts,
                    "SAVE AS CUSTOM LEVEL",
                    MenuCommand::SaveEditorLevel,
                );
            }
        });
    if let Some(message) = &menu.message {
        commands.entity(panel).with_children(|panel| {
            panel.spawn(fonts.text(message.clone(), 15.0, GOOD));
        });
    }
}

fn select_entry(click: On<PointerClick>, buttons: Query<&ListButton>, mut menu: ResMut<MainMenu>) {
    if let Ok(button) = buttons.get(click.entity) {
        menu.selection = button.0;
        menu.message = None;
        menu.needs_rebuild = true;
    }
}

fn change_option(
    click: On<PointerClick>,
    fields: Query<&EditorOption>,
    mut menu: ResMut<MainMenu>,
) {
    let Ok(field) = fields.get(click.entity) else {
        return;
    };
    let forward = click.button != PointerButton::Secondary;
    field.0.change(&mut menu.options, forward);
    menu.message = None;
    menu.needs_rebuild = true;
}

fn start_level(
    commands: &mut Commands,
    next_state: &mut NextState<GameState>,
    level: LevelDefinition,
) {
    commands.insert_resource(ActiveLevel(level));
    next_state.set(GameState::Playing);
}

fn run_command(
    click: On<PointerClick>,
    mut commands: Commands,
    buttons: Query<&MenuCommand>,
    mut menu: ResMut<MainMenu>,
    mut requests: ResMut<SaveRequests>,
    mut next_state: ResMut<NextState<GameState>>,
    mut exit: MessageWriter<AppExit>,
) {
    let Ok(&command) = buttons.get(click.entity) else {
        return;
    };
    if click.button != PointerButton::Primary {
        return;
    }
    match command {
        MenuCommand::StartSelected => {
            if let Selection::Level(index) = menu.selection {
                start_level(
                    &mut commands,
                    &mut next_state,
                    menu.entries[index].level.clone(),
                );
            }
        }
        MenuCommand::StartEditorBattle => {
            start_level(&mut commands, &mut next_state, menu.options.to_level())
        }
        MenuCommand::SaveEditorLevel => {
            let level = menu.options.to_level();
            menu.message = Some(
                match save_custom_level(&custom_levels_directory(), &level) {
                    Ok(path) => format!(
                        "Saved as {} - it now appears under Custom levels.",
                        path.display()
                    ),
                    Err(error) => format!("Could not save the level: {error}"),
                },
            );
            menu.entries = all_levels();
            menu.needs_rebuild = true;
        }
        MenuCommand::ContinueQuickSave => requests.load = Some(QUICK_SAVE_SLOT),
        MenuCommand::Quit => {
            exit.write(AppExit::Success);
        }
    }
}

pub fn refresh_main_menu(
    mut commands: Commands,
    fonts: Res<UiFonts>,
    menu: Option<ResMut<MainMenu>>,
    lists: Query<Entity, With<LevelList>>,
    panels: Query<Entity, With<DetailPanel>>,
    mut buttons: Query<
        (&Hovered, &mut BackgroundColor, Option<&ListButton>),
        Or<(With<ListButton>, With<MenuCommand>, With<EditorOption>)>,
    >,
) {
    let Some(mut menu) = menu else { return };
    if menu.needs_rebuild
        && let (Ok(list), Ok(panel)) = (lists.single(), panels.single())
    {
        menu.needs_rebuild = false;
        rebuild_level_list(&mut commands, list, &fonts, &menu);
        rebuild_details(&mut commands, panel, &fonts, &menu);
    }
    for (hovered, mut background, list_button) in &mut buttons {
        let selected = list_button.is_some_and(|button| button.0 == menu.selection);
        background.0 = if selected {
            BUTTON_SELECTED
        } else if hovered.get() {
            BUTTON_HOVERED
        } else {
            BUTTON_IDLE
        };
    }
}
