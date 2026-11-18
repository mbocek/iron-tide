use bevy::picking::hover::Hovered;
use bevy::prelude::*;

use crate::catalog::Team;
use crate::i18n::{Language, set_language, tr, tr_fmt, tr_owned};
use crate::levels::{
    ActiveLevel, BaseSite, Difficulty, LevelDefinition, LevelEntry, LevelSource, ObjectiveKind,
    SkirmishOptions, TechLevel, Weather, campaign, custom_levels, custom_levels_directory,
    save_custom_level,
};
use crate::lobby::{NetLobby, network_level};
use crate::map::MapSize;
use crate::match_flow::GameState;
use crate::replay::{ReplayPlayback, ReplaySummary, list_replays, load_replay, replay_directory};
use crate::savegame::{QUICK_SAVE_SLOT, SaveDirectory, SaveRequests, slot_summary};
use crate::season::Season;
use crate::settings::GameSettings;
use crate::terrain::Landscape;
use crate::ui::{
    ACCENT, FRAME, FRAME_BRIGHT, GOOD, PANEL, TEXT_DIM, TEXT_MAIN, UiFonts, set_field,
};
use crate::updates::{UpdateState, Updates, download_update};

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
    Network,
    Replay(usize),
}

#[derive(Resource)]
pub struct MainMenu {
    entries: Vec<LevelEntry>,
    replays: Vec<ReplaySummary>,
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

#[derive(Component)]
pub struct JoinAddressText;

#[derive(Component)]
pub struct NetworkStatusText;

#[derive(Component, Clone, Copy)]
pub enum MenuCommand {
    StartSelected,
    StartEditorBattle,
    SaveEditorLevel,
    WatchReplay,
    CycleLanguage,
    ContinueQuickSave,
    DownloadUpdate,
    HostNetworkBattle,
    JoinNetworkBattle,
    AcceptNetworkOffer,
    CancelNetworkBattle,
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
    Alliance,
    Landscape,
    Weather,
    Season,
    MapSize,
}

const EDITOR_FIELDS: [EditorField; 13] = [
    EditorField::Enemies,
    EditorField::Difficulty,
    EditorField::Credits,
    EditorField::Objective,
    EditorField::Tech,
    EditorField::MapVariant,
    EditorField::PlayerSite,
    EditorField::FogOfWar,
    EditorField::Alliance,
    EditorField::Landscape,
    EditorField::Weather,
    EditorField::Season,
    EditorField::MapSize,
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
            EditorField::Enemies => tr("Enemy commanders"),
            EditorField::Difficulty => tr("Difficulty"),
            EditorField::Credits => tr("Starting credits"),
            EditorField::Objective => tr("Objective"),
            EditorField::Tech => tr("Available units"),
            EditorField::MapVariant => tr("Map variant"),
            EditorField::PlayerSite => tr("Your base"),
            EditorField::FogOfWar => tr("Fog of war"),
            EditorField::Alliance => tr("Opponents"),
            EditorField::Landscape => tr("Landscape"),
            EditorField::Weather => tr("Weather"),
            EditorField::Season => tr("Season"),
            EditorField::MapSize => tr("Map size"),
        }
    }

    fn value(self, options: &SkirmishOptions) -> String {
        match self {
            EditorField::Enemies => options.enemies.to_string(),
            EditorField::Difficulty => tr_owned(&format!("{:?}", options.difficulty)),
            EditorField::Credits => format!("${}", options.credits),
            EditorField::Objective => match options.objective {
                ObjectiveKind::Annihilate => tr("Destroy everything").into(),
                ObjectiveKind::Decapitate => tr("Destroy Command Centers").into(),
                ObjectiveKind::Survive => tr("Survive 10 minutes").into(),
                ObjectiveKind::Economy => tr("Stockpile credits").into(),
            },
            EditorField::Tech => options.tech.label().into(),
            EditorField::MapVariant => tr_fmt("{} of {}", &[&options.map_variant, &MAP_VARIANTS]),
            EditorField::PlayerSite => site_name(options.player_site).into(),
            EditorField::FogOfWar => if options.fog_of_war {
                tr("On")
            } else {
                tr("Off")
            }
            .into(),
            EditorField::Alliance => if options.free_for_all {
                tr("Free-for-all")
            } else {
                tr("Allied")
            }
            .into(),
            EditorField::Landscape => tr_owned(&format!("{:?}", options.landscape)),
            EditorField::Weather => tr_owned(&format!("{:?}", options.weather)),
            EditorField::Season => tr_owned(&format!("{:?}", options.season)),
            EditorField::MapSize => tr_fmt(
                "{} ({} x {} cells)",
                &[
                    &tr_owned(&format!("{:?}", options.map_size)),
                    &options.map_size.cells(),
                    &options.map_size.cells(),
                ],
            ),
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
            EditorField::Alliance => options.free_for_all = !options.free_for_all,
            EditorField::Landscape => {
                options.landscape = cycle(&Landscape::ALL, options.landscape, forward);
            }
            EditorField::Weather => {
                options.weather = cycle(&[Weather::Clear, Weather::Rain], options.weather, forward);
            }
            EditorField::Season => {
                options.season = cycle(&Season::ALL, options.season, forward);
            }
            EditorField::MapSize => {
                options.map_size = cycle(&MapSize::ALL, options.map_size, forward);
            }
        }
    }
}

fn site_name(site: BaseSite) -> &'static str {
    match site {
        BaseSite::SouthWest => tr("South-west"),
        BaseSite::NorthEast => tr("North-east"),
        BaseSite::NorthWest => tr("North-west"),
        BaseSite::SouthEast => tr("South-east"),
    }
}

fn minutes(secs: f32) -> String {
    format!("{}:{:02}", secs as u32 / 60, secs as u32 % 60)
}

fn all_levels() -> Vec<LevelEntry> {
    let mut entries = campaign();
    entries.extend(custom_levels(&custom_levels_directory()));
    entries
}

pub fn build_main_menu(
    settings: Res<GameSettings>,
    mut commands: Commands,
    fonts: Res<UiFonts>,
    save_directory: Res<SaveDirectory>,
) {
    let menu = MainMenu {
        entries: all_levels(),
        replays: list_replays(&replay_directory()),
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
                column.spawn(fonts.text(tr("Coalition command"), 18.0, Team::Player.ui_color()));
                column
                    .spawn((
                        MenuCommand::DownloadUpdate,
                        UpdateBanner,
                        Hovered::default(),
                        Node {
                            display: Display::None,
                            ..button_node(Val::Percent(100.0))
                        },
                        BackgroundColor(BUTTON_IDLE),
                        BorderColor::all(GOOD),
                    ))
                    .with_children(|button| {
                        button.spawn(fonts.heading("", 16.0, GOOD));
                    })
                    .observe(run_command);
                if let Some(summary) = has_quick_save {
                    spawn_command_button(
                        column,
                        &fonts,
                        &tr_fmt("Continue  ({})", &[&summary]),
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
                let language = format!("{}: {}", tr("Language"), settings.language.native_name());
                spawn_command_button(column, &fonts, &language, MenuCommand::CycleLanguage);
                spawn_command_button(column, &fonts, tr("Quit to desktop"), MenuCommand::Quit);
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

#[derive(Component)]
pub struct UpdateBanner;

fn update_banner_label(state: &UpdateState) -> Option<String> {
    match state {
        UpdateState::NothingNew => None,
        UpdateState::Available(release) => Some(tr_fmt(
            "Version {} is out - download it",
            &[&release.version],
        )),
        UpdateState::Downloading { release, fraction } => Some(tr_fmt(
            "Downloading {}... {}%",
            &[&release.version, &((fraction * 100.0) as u32)],
        )),
        UpdateState::Downloaded(path) => {
            Some(tr_fmt("Saved to {} - click to show it", &[&path.display()]))
        }
        UpdateState::Failed(reason) => Some(tr_fmt("Update failed: {}", &[&reason])),
    }
}

pub fn refresh_update_banner(
    updates: Res<Updates>,
    mut banners: Query<(&mut Node, &Children), With<UpdateBanner>>,
    mut labels: Query<&mut Text>,
) {
    let label = update_banner_label(&updates.state);
    let display = if label.is_some() {
        Display::Flex
    } else {
        Display::None
    };
    for (mut node, children) in &mut banners {
        set_field(&mut node, |node| &mut node.display, display);
        if let Some(mut text) = children
            .first()
            .and_then(|&child| labels.get_mut(child).ok())
        {
            text.set_if_neq(Text(label.clone().unwrap_or_default()));
        }
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
        list.spawn(fonts.heading(tr("CAMPAIGN"), 15.0, TEXT_DIM));
        for (index, entry) in menu.entries.iter().enumerate() {
            if index > 0
                && matches!(entry.source, LevelSource::Custom(_))
                && !matches!(menu.entries[index - 1].source, LevelSource::Custom(_))
            {
                list.spawn(fonts.heading(tr("CUSTOM LEVELS"), 15.0, TEXT_DIM));
            }
            spawn_list_button(list, fonts, &entry.level.name, Selection::Level(index));
        }
        if !menu
            .entries
            .iter()
            .any(|entry| matches!(entry.source, LevelSource::Custom(_)))
        {
            list.spawn(fonts.text(
                tr("No custom levels yet - make one in the battle editor."),
                14.0,
                TEXT_DIM,
            ));
        }
        list.spawn(fonts.heading(tr("QUICK BATTLE"), 15.0, TEXT_DIM));
        spawn_list_button(list, fonts, tr("Battle editor"), Selection::Editor);
        list.spawn(fonts.heading(tr("NETWORK"), 15.0, TEXT_DIM));
        spawn_list_button(list, fonts, tr("Network battle"), Selection::Network);
        if !menu.replays.is_empty() {
            list.spawn(fonts.heading(tr("REPLAYS"), 15.0, TEXT_DIM));
        }
        for (index, replay) in menu.replays.iter().enumerate() {
            let label = format!("{}  ({})", replay.title, minutes(replay.recorded_secs));
            spawn_list_button(list, fonts, &label, Selection::Replay(index));
        }
    });
}

fn describe_level(level: &LevelDefinition) -> Vec<(String, Color)> {
    let mut lines = vec![
        (level.briefing.clone(), TEXT_MAIN),
        (String::new(), TEXT_DIM),
    ];
    lines.push((
        tr_fmt("Objective: {}", &[&level.objective.describe()]),
        ACCENT,
    ));
    lines.push((
        tr_fmt(
            "Map: {}, {}, {} x {} cells",
            &[
                &tr_owned(&format!("{:?}", level.landscape)),
                &tr_owned(&format!("{:?}", level.season)),
                &level.map_size.cells(),
                &level.map_size.cells(),
            ],
        ),
        TEXT_MAIN,
    ));
    lines.push((
        tr_fmt(
            "Your base: {}  -  ${}",
            &[&site_name(level.player.site), &level.player.credits],
        ),
        Team::Player.ui_color(),
    ));
    for (team, enemy) in level.opponents() {
        lines.push((
            format!(
                "{}: {}, {}, ${}",
                tr(team.name()),
                site_name(enemy.force.site),
                tr_owned(&format!("{:?}", enemy.difficulty)),
                enemy.force.credits
            ),
            team.ui_color(),
        ));
    }
    let units = if level.available_units.is_empty() {
        tr("every unit").to_string()
    } else {
        level
            .available_units
            .iter()
            .map(|kind| tr(kind.spec().name))
            .collect::<Vec<_>>()
            .join(", ")
    };
    lines.push((tr_fmt("Available: {}", &[&units]), TEXT_DIM));
    lines
}

const ADDRESS_CHARACTERS: &str = "0123456789.:-abcdefghijklmnopqrstuvwxyz";
const LONGEST_ADDRESS: usize = 64;

impl MainMenu {
    pub fn show_network_page(&mut self) {
        if self.selection != Selection::Network {
            self.selection = Selection::Network;
            self.needs_rebuild = true;
        }
    }
}

/// Typing into the host address field while the network battle page is shown.
pub fn type_join_address(
    menu: Option<Res<MainMenu>>,
    mut typed: MessageReader<bevy::input::keyboard::KeyboardInput>,
    mut settings: ResMut<GameSettings>,
    mut fields: Query<&mut Text, With<JoinAddressText>>,
) {
    let on_network_page = menu.is_some_and(|menu| menu.selection == Selection::Network);
    if !on_network_page {
        typed.clear();
        return;
    }
    let mut address = settings.join_address.clone();
    for key in typed.read().filter(|key| key.state.is_pressed()) {
        match &key.logical_key {
            bevy::input::keyboard::Key::Backspace => {
                address.pop();
            }
            bevy::input::keyboard::Key::Character(text) => {
                for character in text.to_lowercase().chars() {
                    if ADDRESS_CHARACTERS.contains(character) && address.len() < LONGEST_ADDRESS {
                        address.push(character);
                    }
                }
            }
            _ => {}
        }
    }
    if address != settings.join_address {
        settings.join_address = address.clone();
        for mut field in &mut fields {
            field.0 = address.clone();
        }
    }
}

pub fn show_network_status(
    time: Res<Time<Real>>,
    lobby: Res<NetLobby>,
    mut lines: Query<&mut Text, With<NetworkStatusText>>,
) {
    let status = lobby.status(time.elapsed_secs()).unwrap_or_default();
    for mut line in &mut lines {
        line.set_if_neq(Text(status.clone()));
    }
}

fn spawn_network_details(
    panel: &mut ChildSpawnerCommands,
    fonts: &UiFonts,
    join_address: &str,
    lobby: &NetLobby,
) {
    panel.spawn(fonts.heading(tr("Network battle"), 30.0, ACCENT));
    panel.spawn(fonts.text(
        tr("Two players on two computers, one against one. The host plays the Coalition with the battle editor's settings; the other player joins as the Syndicate. Both need the same version of the game, and the host's port 47300 must be reachable."),
        15.0,
        TEXT_DIM,
    ));
    spawn_command_button(
        panel,
        fonts,
        tr("HOST A BATTLE"),
        MenuCommand::HostNetworkBattle,
    );
    panel
        .spawn(Node {
            justify_content: JustifyContent::SpaceBetween,
            align_items: AlignItems::Center,
            ..default()
        })
        .with_children(|row| {
            row.spawn(fonts.text(tr("Host address (type it)"), 18.0, TEXT_MAIN));
            row.spawn((
                button_node(Val::Px(260.0)),
                BackgroundColor(BUTTON_IDLE),
                BorderColor::all(ACCENT),
            ))
            .with_children(|field| {
                field.spawn((JoinAddressText, fonts.heading(join_address, 16.0, ACCENT)));
            });
        });
    spawn_command_button(panel, fonts, tr("JOIN"), MenuCommand::JoinNetworkBattle);
    panel.spawn((NetworkStatusText, fonts.text("", 16.0, GOOD)));
    if let Some(level) = lobby.offered_level() {
        for (line, color) in describe_level(level).into_iter().skip(2) {
            panel.spawn(fonts.text(line, 15.0, color));
        }
    }
    if lobby.can_accept_offer() {
        spawn_command_button(
            panel,
            fonts,
            tr("I'M READY"),
            MenuCommand::AcceptNetworkOffer,
        );
    }
    if lobby.is_busy() {
        spawn_command_button(panel, fonts, tr("LEAVE"), MenuCommand::CancelNetworkBattle);
    }
}

fn rebuild_details(
    commands: &mut Commands,
    panel: Entity,
    fonts: &UiFonts,
    menu: &MainMenu,
    join_address: &str,
    lobby: &NetLobby,
) {
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
                spawn_command_button(panel, fonts, tr("START MISSION"), MenuCommand::StartSelected);
            }
            Selection::Replay(index) => {
                let replay = &menu.replays[index];
                panel.spawn(fonts.heading(&replay.title, 30.0, ACCENT));
                panel.spawn(fonts.text(
                    format!("{}  ·  {}", tr_owned(&replay.outcome), minutes(replay.recorded_secs)),
                    17.0,
                    TEXT_MAIN,
                ));
                panel.spawn(fonts.text(
                    tr(
                        "Watch the whole battle again without the fog of war. Space pauses, + and - change the speed, Esc returns here.",
                    ),
                    15.0,
                    TEXT_DIM,
                ));
                spawn_command_button(panel, fonts, tr("WATCH REPLAY"), MenuCommand::WatchReplay);
            }
            Selection::Network => spawn_network_details(panel, fonts, join_address, lobby),
            Selection::Editor => {
                panel.spawn(fonts.heading(tr("Battle editor"), 30.0, ACCENT));
                panel.spawn(fonts.text(
                    tr("Left click: next value, right click: previous."),
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
                spawn_command_button(panel, fonts, tr("START BATTLE"), MenuCommand::StartEditorBattle);
                spawn_command_button(
                    panel,
                    fonts,
                    tr("SAVE AS CUSTOM LEVEL"),
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
    mut settings: ResMut<GameSettings>,
    mut updates: ResMut<Updates>,
    mut lobby: ResMut<NetLobby>,
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
                    Ok(path) => tr_fmt(
                        "Saved as {} - it now appears under Custom levels.",
                        &[&path.display()],
                    ),
                    Err(error) => tr_fmt("Could not save the level: {}", &[&error]),
                },
            );
            menu.entries = all_levels();
            menu.needs_rebuild = true;
        }
        MenuCommand::WatchReplay => {
            let Selection::Replay(index) = menu.selection else {
                return;
            };
            match load_replay(&menu.replays[index].path) {
                Ok(replay) => {
                    commands.insert_resource(ActiveLevel(replay.level.clone()));
                    commands.insert_resource(ReplayPlayback::new(replay));
                    next_state.set(GameState::Replay);
                }
                Err(error) => {
                    menu.message = Some(tr_fmt("Could not open the replay: {}", &[&error]));
                    menu.needs_rebuild = true;
                }
            }
        }
        MenuCommand::CycleLanguage => {
            let next = Language::ALL
                .iter()
                .position(|&language| language == settings.language)
                .map_or(0, |index| (index + 1) % Language::ALL.len());
            settings.language = Language::ALL[next];
            set_language(settings.language);
            next_state.set(GameState::MainMenu);
        }
        MenuCommand::ContinueQuickSave => requests.load = Some(QUICK_SAVE_SLOT),
        MenuCommand::DownloadUpdate => download_update(&mut updates),
        MenuCommand::HostNetworkBattle => lobby.host(network_level(&menu.options)),
        MenuCommand::JoinNetworkBattle => lobby.join(&settings.join_address),
        MenuCommand::AcceptNetworkOffer => lobby.accept_offer(),
        MenuCommand::CancelNetworkBattle => lobby.cancel(),
        MenuCommand::Quit => {
            exit.write(AppExit::Success);
        }
    }
}

pub fn refresh_main_menu(
    mut commands: Commands,
    fonts: Res<UiFonts>,
    settings: Res<GameSettings>,
    mut lobby: ResMut<NetLobby>,
    menu: Option<ResMut<MainMenu>>,
    lists: Query<Entity, With<LevelList>>,
    panels: Query<Entity, With<DetailPanel>>,
    mut buttons: Query<
        (&Hovered, &mut BackgroundColor, Option<&ListButton>),
        Or<(With<ListButton>, With<MenuCommand>, With<EditorOption>)>,
    >,
) {
    let Some(mut menu) = menu else { return };
    if lobby.take_change() && menu.selection == Selection::Network {
        menu.needs_rebuild = true;
    }
    if menu.needs_rebuild
        && let (Ok(list), Ok(panel)) = (lists.single(), panels.single())
    {
        menu.needs_rebuild = false;
        rebuild_level_list(&mut commands, list, &fonts, &menu);
        rebuild_details(
            &mut commands,
            panel,
            &fonts,
            &menu,
            &settings.join_address,
            &lobby,
        );
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
