use crate::campaigns::{
    Campaign, CampaignLibrary, CampaignRun, add_to_my_campaign, built_in_campaigns,
    custom_campaigns, find_campaign,
};
use crate::map_editor::open_map_editor;
use crate::map_preview::map_preview;
use crate::menu::{MenuPlace, MenuState, close_settings, open_settings, spawn_menu};
use crate::mission_generator::generate_mission;
use crate::network::NetSession;
use crate::outcome::GameOutcome;
use crate::random_battle::{
    self, CREDIT_CHOICES, DifficultyChoice, FactionChoice, RandomBattleOptions,
};
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use rand::RngExt;

use crate::catalog::Team;
use crate::catalog::{Alliance, Faction};
use crate::i18n::{Language, set_language, tr, tr_fmt, tr_owned};
use crate::levels::{
    ActiveLevel, BaseSite, Difficulty, LevelDefinition, LevelEntry, LevelSource, ObjectiveKind,
    SkirmishOptions, TechLevel, Weather, custom_levels, custom_levels_directory, save_custom_level,
};
use crate::lobby::{MOST_PLAYERS, NetLobby, network_level};
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

const NAV_WIDTH: f32 = 240.0;
const LIST_WIDTH: f32 = 330.0;
const DETAIL_WIDTH: f32 = 640.0;
const REPLAYS_LISTED: usize = 10;
const LARGE_PREVIEW: f32 = 220.0;
const FACT_LABEL_WIDTH: f32 = 130.0;
const SMALL_PREVIEW: f32 = 170.0;
const BODY_GAP: f32 = 14.0;
const EDITOR_FIELD_WIDTH: f32 = 440.0;
const EDITOR_VALUE_WIDTH: f32 = 230.0;
const BUTTON_IDLE: Color = Color::srgb(0.13, 0.15, 0.17);
const BUTTON_HOVERED: Color = Color::srgb(0.19, 0.22, 0.25);
const BUTTON_SELECTED: Color = Color::srgb(0.17, 0.24, 0.32);
const BACKDROP: Color = Color::srgb(0.035, 0.045, 0.055);
/// A battle scene behind the menu, darkened by the backdrop colour at this opacity so text stays readable.
const BACKDROP_IMAGE: &str = "ui/menu_backdrop.jpg";
const BACKDROP_VEIL: f32 = 0.8;
const PRIMARY_IDLE: Color = Color::srgb(0.62, 0.45, 0.10);
const PRIMARY_HOVERED: Color = Color::srgb(0.76, 0.56, 0.14);
const MAP_VARIANTS: u64 = 5;
const HUB_TILE_WIDTH: f32 = 420.0;
const HUB_TILE_HEIGHT: f32 = 120.0;
const HUB_GAP: f32 = 16.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Selection {
    Level(usize),
    /// A mission of the campaign at that index in the campaign list.
    Mission {
        campaign: usize,
        mission: usize,
    },
    Editor,
    Network,
    Replay(usize),
    Quick,
    Generated,
    Random,
    /// A section with nothing in it yet.
    Nothing(MenuSection),
}

/// The groups along the left edge of the main menu; each shows its own list or page.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MenuSection {
    Campaign,
    Quick,
    Generated,
    Random,
    Custom,
    Editor,
    Network,
    Replays,
}

impl MenuSection {
    fn label(self) -> &'static str {
        match self {
            MenuSection::Campaign => tr("CAMPAIGN"),
            MenuSection::Quick => tr("QUICK BATTLE"),
            MenuSection::Generated => tr("MISSION GENERATOR"),
            MenuSection::Random => tr("RANDOM BATTLE"),
            MenuSection::Custom => tr("CUSTOM LEVELS"),
            MenuSection::Editor => tr("BATTLE EDITOR"),
            MenuSection::Network => tr("NETWORK BATTLE"),
            MenuSection::Replays => tr("REPLAYS"),
        }
    }

    /// Sections that are a single page need no list beside them.
    fn has_list(self) -> bool {
        !matches!(
            self,
            MenuSection::Quick
                | MenuSection::Generated
                | MenuSection::Random
                | MenuSection::Editor
                | MenuSection::Network
        )
    }
}

/// The tiles of the start screen; each opens a screen with the sections that belong to it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MenuGroup {
    Campaigns,
    Skirmish,
    Multiplayer,
    Editor,
    Replays,
}

impl MenuGroup {
    const ALL: [MenuGroup; 5] = [
        MenuGroup::Campaigns,
        MenuGroup::Skirmish,
        MenuGroup::Multiplayer,
        MenuGroup::Editor,
        MenuGroup::Replays,
    ];

    fn title(self) -> &'static str {
        match self {
            MenuGroup::Campaigns => tr("CAMPAIGNS"),
            MenuGroup::Skirmish => tr("SKIRMISH"),
            MenuGroup::Multiplayer => tr("MULTIPLAYER"),
            MenuGroup::Editor => tr("EDITOR"),
            MenuGroup::Replays => tr("REPLAYS"),
        }
    }

    fn blurb(self) -> &'static str {
        match self {
            MenuGroup::Campaigns => tr("Story missions for the Coalition and the Syndicate"),
            MenuGroup::Skirmish => tr("A battle against the computer, rolled or set up your way"),
            MenuGroup::Multiplayer => tr("Battles over the network, up to four players"),
            MenuGroup::Editor => tr("Set up battles, paint maps and play your own levels"),
            MenuGroup::Replays => tr("Watch your recent matches again"),
        }
    }

    fn sections(self) -> &'static [MenuSection] {
        match self {
            MenuGroup::Campaigns => &[MenuSection::Campaign],
            MenuGroup::Skirmish => &[
                MenuSection::Quick,
                MenuSection::Generated,
                MenuSection::Random,
                MenuSection::Custom,
            ],
            MenuGroup::Multiplayer => &[MenuSection::Network],
            MenuGroup::Editor => &[MenuSection::Editor, MenuSection::Custom],
            MenuGroup::Replays => &[MenuSection::Replays],
        }
    }

    fn holding(section: MenuSection) -> MenuGroup {
        MenuGroup::ALL
            .into_iter()
            .find(|group| group.sections().contains(&section))
            .unwrap_or(MenuGroup::Campaigns)
    }
}

/// What the main menu shows: the start screen, or one group's sections.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Screen {
    Hub,
    Group(MenuGroup),
}

#[derive(Component)]
pub struct SectionButton(MenuSection);

#[derive(Component)]
pub struct HubTile(MenuGroup);

/// Holds the current screen's contents; rebuilt whenever the screen changes.
#[derive(Component)]
pub struct ScreenBody;

#[derive(Resource)]
pub struct MainMenu {
    campaigns: Vec<Campaign>,
    campaign_difficulty: Difficulty,
    quick_faction: FactionChoice,
    quick_difficulty: Difficulty,
    screen: Screen,
    needs_new_screen: bool,
    entries: Vec<LevelEntry>,
    replays: Vec<ReplaySummary>,
    selection: Selection,
    options: SkirmishOptions,
    random: RandomBattleOptions,
    /// The dice of the random battle on show; a new roll draws another.
    random_seed: u64,
    completed_missions: Vec<String>,
    /// The map picture of the level last shown, kept until another level is shown.
    preview: Option<(LevelDefinition, Handle<Image>)>,
    message: Option<String>,
    needs_rebuild: bool,
}

#[derive(Component)]
pub struct MainMenuRoot;

#[derive(Component)]
pub struct LevelList;

#[derive(Component)]
pub struct ListColumn;

#[derive(Component)]
pub struct DetailPanel;

#[derive(Component)]
pub struct ListButton(Selection);

#[derive(Component)]
pub struct EditorOption(EditorField);

#[derive(Component)]
pub struct RandomOption(RandomField);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RandomField {
    Opponents,
    Difficulty,
    MapSize,
    Credits,
    Faction,
}

impl RandomField {
    const ALL: [RandomField; 5] = [
        RandomField::Opponents,
        RandomField::Difficulty,
        RandomField::MapSize,
        RandomField::Credits,
        RandomField::Faction,
    ];

    fn label(self) -> &'static str {
        match self {
            RandomField::Opponents => tr("Enemy commanders"),
            RandomField::Difficulty => tr("Difficulty"),
            RandomField::MapSize => tr("Map size"),
            RandomField::Credits => tr("Starting credits"),
            RandomField::Faction => tr("Your faction"),
        }
    }

    fn value(self, options: &RandomBattleOptions) -> String {
        match self {
            RandomField::Opponents => options.opponents.to_string(),
            RandomField::Difficulty => match options.difficulty {
                DifficultyChoice::Fixed(difficulty) => tr_owned(&format!("{difficulty:?}")),
                DifficultyChoice::Mixed => tr("Mixed").into(),
            },
            RandomField::MapSize => tr_fmt(
                "{} ({} x {} cells)",
                &[
                    &tr_owned(&format!("{:?}", options.map_size)),
                    &options.map_size.cells(),
                    &options.map_size.cells(),
                ],
            ),
            RandomField::Credits => format!("${}", options.credits),
            RandomField::Faction => match options.faction {
                FactionChoice::Fixed(faction) => tr(Team::Player.name_in(faction)).into(),
                FactionChoice::Random => tr("Random").into(),
            },
        }
    }

    fn change(self, options: &mut RandomBattleOptions, forward: bool) {
        match self {
            RandomField::Opponents => {
                options.opponents = cycle(&[1, 2, 3], options.opponents, forward)
            }
            RandomField::Difficulty => {
                options.difficulty = cycle(&DifficultyChoice::ALL, options.difficulty, forward)
            }
            RandomField::MapSize => {
                options.map_size = cycle(&MapSize::ALL, options.map_size, forward)
            }
            RandomField::Credits => {
                options.credits = cycle(&CREDIT_CHOICES, options.credits, forward)
            }
            RandomField::Faction => {
                let choices = [
                    FactionChoice::Fixed(Faction::Coalition),
                    FactionChoice::Fixed(Faction::Syndicate),
                    FactionChoice::Random,
                ];
                options.faction = cycle(&choices, options.faction, forward);
            }
        }
    }
}

#[derive(Component)]
pub struct JoinAddressText;

#[derive(Component)]
pub struct NetworkStatusText;

#[derive(Component)]
pub struct ChatDraftText;

#[derive(Component, Clone, Copy)]
pub enum MenuCommand {
    StartSelected,
    QuickFaction(FactionChoice),
    QuickDifficulty(Difficulty),
    StartQuickBattle,
    StartEditorBattle,
    SaveEditorLevel,
    EditMap,
    RerollRandomBattle,
    WatchReplay,
    CycleLanguage,
    ContinueQuickSave,
    StartCampaignMission,
    CycleCampaignDifficulty,
    ContinueCampaign,
    AddToMyCampaign,
    OpenSettings,
    BackToHub,
    DownloadUpdate,
    HostNetworkBattle,
    JoinNetworkBattle,
    OpenServerRoom,
    AcceptNetworkOffer,
    StartNetworkBattle,
    CancelNetworkBattle,
    Quit,
}

impl MenuCommand {
    /// The action a page exists for, drawn larger and in the accent colour.
    fn is_primary(self) -> bool {
        matches!(
            self,
            MenuCommand::StartSelected
                | MenuCommand::StartQuickBattle
                | MenuCommand::StartCampaignMission
                | MenuCommand::StartEditorBattle
                | MenuCommand::WatchReplay
                | MenuCommand::HostNetworkBattle
                | MenuCommand::AcceptNetworkOffer
                | MenuCommand::StartNetworkBattle
        )
    }
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
    NetworkPlayers,
    Faction,
}

const EDITOR_FIELDS: [EditorField; 15] = [
    EditorField::Enemies,
    EditorField::Difficulty,
    EditorField::Credits,
    EditorField::Objective,
    EditorField::Tech,
    EditorField::MapVariant,
    EditorField::PlayerSite,
    EditorField::Faction,
    EditorField::FogOfWar,
    EditorField::Alliance,
    EditorField::Landscape,
    EditorField::Weather,
    EditorField::Season,
    EditorField::MapSize,
    EditorField::NetworkPlayers,
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
            EditorField::NetworkPlayers => tr("Network players"),
            EditorField::Faction => tr("Your faction"),
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
            EditorField::PlayerSite => options.player_site.name().into(),
            EditorField::FogOfWar => if options.fog_of_war {
                tr("On")
            } else {
                tr("Off")
            }
            .into(),
            EditorField::Alliance => alliance_name(options.alliance).into(),
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
            EditorField::NetworkPlayers => options.network_players.to_string(),
            EditorField::Faction => tr(Team::Player.name_in(options.player_faction)).into(),
        }
    }

    fn change(self, options: &mut SkirmishOptions, forward: bool) {
        match self {
            EditorField::Enemies => options.enemies = cycle(&[1, 2, 3], options.enemies, forward),
            EditorField::Difficulty => {
                options.difficulty = cycle(&Difficulty::ALL, options.difficulty, forward);
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
            EditorField::Alliance => {
                options.alliance = cycle(&Alliance::ALL, options.alliance, forward);
            }
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
            EditorField::NetworkPlayers => {
                let counts: Vec<usize> = (2..=MOST_PLAYERS).collect();
                options.network_players = cycle(&counts, options.network_players, forward);
            }
            EditorField::Faction => options.player_faction = options.player_faction.opposite(),
        }
    }
}

fn minutes(secs: f32) -> String {
    format!("{}:{:02}", secs as u32 / 60, secs as u32 % 60)
}

fn all_levels() -> Vec<LevelEntry> {
    custom_levels(&custom_levels_directory())
}

pub fn build_main_menu(
    settings: Res<GameSettings>,
    mut commands: Commands,
    fonts: Res<UiFonts>,
    asset_server: Res<AssetServer>,
    library: Res<CampaignLibrary>,
) {
    // The menu is built on the first frame, before the language from the saved settings is otherwise applied.
    set_language(settings.language);
    let menu = MainMenu {
        campaigns: library.0.clone(),
        campaign_difficulty: settings.campaign_difficulty,
        quick_faction: FactionChoice::Fixed(Faction::Coalition),
        quick_difficulty: Difficulty::Normal,
        screen: Screen::Hub,
        needs_new_screen: true,
        entries: all_levels(),
        replays: list_replays(&replay_directory()),
        selection: Selection::Nothing(MenuSection::Campaign),
        options: SkirmishOptions::default(),
        random: RandomBattleOptions::default(),
        random_seed: rand::rng().random(),
        completed_missions: settings.completed_missions.clone(),
        preview: None,
        message: None,
        needs_rebuild: true,
    };
    commands.spawn((
        DespawnOnExit(GameState::MainMenu),
        ImageNode::new(asset_server.load(BACKDROP_IMAGE)),
        Node {
            position_type: PositionType::Absolute,
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            ..default()
        },
        GlobalZIndex(-1),
    ));
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
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                padding: UiRect::all(Val::Px(24.0)),
                row_gap: Val::Px(18.0),
                ..default()
            },
            BackgroundColor(BACKDROP.with_alpha(BACKDROP_VEIL)),
        ))
        .with_children(|root| {
            spawn_title_bar(root, &fonts, &settings);
            root.spawn((
                ScreenBody,
                Node {
                    width: Val::Px(full_width()),
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    ..default()
                },
            ));
        });
    spawn_menu(&mut commands, &fonts, MenuPlace::MainMenu);
    commands.insert_resource(menu);
}

fn spawn_title_bar(root: &mut ChildSpawnerCommands, fonts: &UiFonts, settings: &GameSettings) {
    root.spawn(Node {
        width: Val::Px(full_width()),
        justify_content: JustifyContent::SpaceBetween,
        align_items: AlignItems::Center,
        column_gap: Val::Px(10.0),
        ..default()
    })
    .with_children(|bar| {
        bar.spawn(Node {
            flex_direction: FlexDirection::Column,
            ..default()
        })
        .with_children(|title| {
            title.spawn(fonts.heading("IRON TIDE", 46.0, ACCENT));
            title.spawn(fonts.text(
                format!(
                    "{}  ·  v{}",
                    tr("Real-time strategy"),
                    env!("CARGO_PKG_VERSION")
                ),
                15.0,
                TEXT_DIM,
            ));
        });
        bar.spawn(Node {
            column_gap: Val::Px(8.0),
            align_items: AlignItems::Center,
            ..default()
        })
        .with_children(|actions| {
            actions
                .spawn((
                    MenuCommand::DownloadUpdate,
                    UpdateBanner,
                    Hovered::default(),
                    Node {
                        display: Display::None,
                        ..button_node(Val::Auto)
                    },
                    BackgroundColor(BUTTON_IDLE),
                    BorderColor::all(GOOD),
                ))
                .with_children(|button| {
                    button.spawn(fonts.heading("", 16.0, GOOD));
                })
                .observe(run_command);
            let language = format!("{}: {}", tr("Language"), settings.language.native_name());
            spawn_command_button_sized(
                actions,
                fonts,
                &language,
                MenuCommand::CycleLanguage,
                Val::Auto,
            );
            spawn_command_button_sized(
                actions,
                fonts,
                tr("Quit to desktop"),
                MenuCommand::Quit,
                Val::Auto,
            );
        });
    });
}

/// The start screen: a tile for each group, settings, and the quick save to continue from.
fn spawn_hub(
    body: &mut ChildSpawnerCommands,
    fonts: &UiFonts,
    quick_save: Option<String>,
    campaign_to_continue: Option<String>,
) {
    body.spawn(Node {
        flex_direction: FlexDirection::Column,
        row_gap: Val::Px(HUB_GAP),
        margin: UiRect::top(Val::Px(40.0)),
        ..default()
    })
    .with_children(|hub| {
        if let Some(summary) = quick_save {
            hub.spawn((
                MenuCommand::ContinueQuickSave,
                Hovered::default(),
                Node {
                    width: Val::Px(HUB_TILE_WIDTH * 2.0 + HUB_GAP),
                    flex_direction: FlexDirection::Column,
                    ..button_node(Val::Auto)
                },
                BackgroundColor(BUTTON_IDLE),
                BorderColor::all(ACCENT),
            ))
            .with_children(|button| {
                button.spawn(fonts.heading(tr("CONTINUE"), 20.0, ACCENT));
                button.spawn(fonts.text(summary, 14.0, TEXT_DIM));
            })
            .observe(run_command);
        }
        if let Some(summary) = campaign_to_continue {
            hub.spawn((
                MenuCommand::ContinueCampaign,
                Hovered::default(),
                Node {
                    width: Val::Px(HUB_TILE_WIDTH * 2.0 + HUB_GAP),
                    flex_direction: FlexDirection::Column,
                    ..button_node(Val::Auto)
                },
                BackgroundColor(BUTTON_IDLE),
                BorderColor::all(ACCENT),
            ))
            .with_children(|button| {
                button.spawn(fonts.heading(tr("CONTINUE CAMPAIGN"), 20.0, ACCENT));
                button.spawn(fonts.text(summary, 14.0, TEXT_DIM));
            })
            .observe(run_command);
        }
        let tiles: Vec<Option<MenuGroup>> =
            MenuGroup::ALL.into_iter().map(Some).chain([None]).collect();
        for row in tiles.chunks(2) {
            hub.spawn(Node {
                column_gap: Val::Px(HUB_GAP),
                ..default()
            })
            .with_children(|row_node| {
                for &tile in row {
                    let (title, blurb) = match tile {
                        Some(group) => (group.title(), group.blurb()),
                        None => (tr("SETTINGS"), tr("Graphics, sound, controls and gameplay")),
                    };
                    let mut button = row_node.spawn((
                        Hovered::default(),
                        Node {
                            width: Val::Px(HUB_TILE_WIDTH),
                            height: Val::Px(HUB_TILE_HEIGHT),
                            flex_direction: FlexDirection::Column,
                            justify_content: JustifyContent::Center,
                            padding: UiRect::axes(Val::Px(22.0), Val::Px(10.0)),
                            row_gap: Val::Px(6.0),
                            border: UiRect::all(Val::Px(1.0)),
                            ..default()
                        },
                        BackgroundColor(BUTTON_IDLE),
                        BorderColor::all(FRAME),
                    ));
                    button.with_children(|tile_node| {
                        tile_node.spawn(fonts.heading(title, 26.0, TEXT_MAIN));
                        tile_node.spawn(fonts.text(blurb, 15.0, TEXT_DIM));
                    });
                    match tile {
                        Some(group) => {
                            button.insert(HubTile(group)).observe(open_group);
                        }
                        None => {
                            button
                                .insert(MenuCommand::OpenSettings)
                                .observe(run_command);
                        }
                    }
                }
            });
        }
    });
}

/// One group's screen: its sections down the left, a list beside them and the details of what is chosen.
fn spawn_group_screen(body: &mut ChildSpawnerCommands, fonts: &UiFonts, group: MenuGroup) {
    body.spawn(Node {
        width: Val::Px(full_width()),
        column_gap: Val::Px(BODY_GAP),
        align_items: AlignItems::FlexStart,
        ..default()
    })
    .with_children(|body| {
        spawn_section_column(body, fonts, group);
        body.spawn((
            ListColumn,
            Node {
                width: Val::Px(LIST_WIDTH),
                flex_shrink: 0.0,
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(14.0)),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(PANEL),
            BorderColor::all(FRAME),
        ))
        .with_children(|column| {
            column.spawn((
                LevelList,
                Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(5.0),
                    ..default()
                },
            ));
        });
        body.spawn((
            DetailPanel,
            Node {
                min_width: Val::Px(DETAIL_WIDTH),
                flex_grow: 1.0,
                min_height: Val::Px(420.0),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(20.0)),
                row_gap: Val::Px(8.0),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(PANEL),
            BorderColor::all(FRAME_BRIGHT),
        ));
    });
}

fn open_group(click: On<PointerClick>, tiles: Query<&HubTile>, mut menu: ResMut<MainMenu>) {
    if let Ok(tile) = tiles.get(click.entity) {
        menu.open_group(tile.0);
    }
}

/// Puts the screen the menu is on into the screen body, after the screen changed.
pub fn rebuild_screen(
    mut commands: Commands,
    fonts: Res<UiFonts>,
    save_directory: Res<SaveDirectory>,
    settings: Res<GameSettings>,
    menu: Option<ResMut<MainMenu>>,
    bodies: Query<Entity, With<ScreenBody>>,
) {
    let (Some(mut menu), Ok(body)) = (menu, bodies.single()) else {
        return;
    };
    if !menu.needs_new_screen {
        return;
    }
    menu.needs_new_screen = false;
    let continue_summary =
        menu.campaign_to_continue(&settings.last_campaign)
            .map(|(campaign, mission)| {
                let chosen = &menu.campaigns[campaign];
                format!(
                    "{}  ·  {}",
                    tr_owned(&chosen.title),
                    tr_owned(&chosen.missions[mission].name)
                )
            });
    commands.entity(body).despawn_related::<Children>();
    let screen = menu.screen;
    commands.entity(body).with_children(|body| match screen {
        Screen::Hub => spawn_hub(
            body,
            &fonts,
            slot_summary(&save_directory, QUICK_SAVE_SLOT),
            continue_summary.clone(),
        ),
        Screen::Group(group) => spawn_group_screen(body, &fonts, group),
    });
    menu.needs_rebuild = true;
}

/// Escape closes the settings, or leaves a group's screen for the start screen.
pub fn step_back_on_escape(
    keys: Res<ButtonInput<KeyCode>>,
    menu: Option<ResMut<MainMenu>>,
    mut settings_menu: ResMut<MenuState>,
) {
    let Some(mut menu) = menu else { return };
    if !keys.just_pressed(KeyCode::Escape) {
        return;
    }
    if settings_menu.is_open() {
        close_settings(&mut settings_menu);
    } else if menu.screen != Screen::Hub {
        menu.show_hub();
    }
}

const MISSION_NODE: f32 = 34.0;
const MISSION_ROW: f32 = 52.0;
const ROUTE_SWING: f32 = 70.0;

/// The campaign tabs and the route of the chosen campaign's missions, one node per mission joined by a line,
/// zig-zagging down the column: won ones ticked, the next one lit, locked ones dimmed.
fn spawn_operations_map(list: &mut ChildSpawnerCommands, fonts: &UiFonts, menu: &MainMenu) {
    let Selection::Mission {
        campaign: shown,
        mission: chosen,
    } = menu.selection
    else {
        return;
    };
    list.spawn(Node {
        flex_wrap: FlexWrap::Wrap,
        column_gap: Val::Px(6.0),
        row_gap: Val::Px(6.0),
        margin: UiRect::bottom(Val::Px(8.0)),
        ..default()
    })
    .with_children(|tabs| {
        for (index, campaign) in menu.campaigns.iter().enumerate() {
            let progress = format!(
                "{}  {}/{}",
                tr_owned(&campaign.title),
                campaign.won_count(&menu.completed_missions),
                campaign.missions.len()
            );
            tabs.spawn((
                ListButton(menu.campaign_page(index)),
                Hovered::default(),
                button_node(Val::Auto),
                BackgroundColor(BUTTON_IDLE),
                BorderColor::all(if index == shown { ACCENT } else { FRAME }),
            ))
            .with_children(|tab| {
                tab.spawn(fonts.heading(progress, 15.0, TEXT_MAIN));
            })
            .observe(select_entry);
        }
    });
    let campaign = &menu.campaigns[shown];
    let completed = &menu.completed_missions;
    let next = campaign.next_to_play(completed);
    let swing = |mission: usize| {
        if mission.is_multiple_of(2) {
            0.0
        } else {
            ROUTE_SWING
        }
    };
    let height = MISSION_ROW * campaign.missions.len() as f32;
    list.spawn(Node {
        height: Val::Px(height),
        ..default()
    })
    .with_children(|route| {
        for mission in 1..campaign.missions.len() {
            let from = Vec2::new(swing(mission - 1), MISSION_ROW * (mission - 1) as f32);
            let to = Vec2::new(swing(mission), MISSION_ROW * mission as f32);
            for step in 1..8 {
                let at = from.lerp(to, step as f32 / 8.0) + Vec2::splat(MISSION_NODE / 2.0 - 2.0);
                let reached = campaign.won(mission - 1, completed);
                route.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(at.x),
                        top: Val::Px(at.y),
                        width: Val::Px(4.0),
                        height: Val::Px(4.0),
                        ..default()
                    },
                    BackgroundColor(if reached { ACCENT } else { FRAME }),
                ));
            }
        }
        for (mission, level) in campaign.missions.iter().enumerate() {
            let won = campaign.won(mission, completed);
            let unlocked = campaign.unlocked(mission, completed);
            let (mark, mark_color) = match (won, unlocked) {
                (true, _) => (format!("{}", mission + 1), GOOD),
                (false, true) => (format!("{}", mission + 1), ACCENT),
                (false, false) => ("·".to_string(), TEXT_DIM),
            };
            let label_color = if unlocked { TEXT_MAIN } else { TEXT_DIM };
            route
                .spawn((
                    ListButton(Selection::Mission {
                        campaign: shown,
                        mission,
                    }),
                    Hovered::default(),
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(swing(mission)),
                        top: Val::Px(MISSION_ROW * mission as f32),
                        align_items: AlignItems::Center,
                        column_gap: Val::Px(8.0),
                        padding: UiRect::right(Val::Px(8.0)),
                        border: UiRect::all(Val::Px(1.0)),
                        ..default()
                    },
                    BackgroundColor(BUTTON_IDLE),
                    BorderColor::all(if mission == chosen {
                        ACCENT
                    } else if Some(mission) == next {
                        GOOD
                    } else {
                        FRAME
                    }),
                ))
                .with_children(|node| {
                    node.spawn(Node {
                        width: Val::Px(MISSION_NODE),
                        height: Val::Px(MISSION_NODE),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        ..default()
                    })
                    .with_children(|badge| {
                        badge.spawn(fonts.heading(mark, 18.0, mark_color));
                    });
                    node.spawn(fonts.heading(short_mission_name(level), 15.0, label_color));
                    if won {
                        node.spawn(fonts.heading(tr("WON"), 12.0, GOOD));
                    }
                })
                .observe(select_entry);
        }
    });
}

/// A mission's name without the number its title starts with; the node shows the number.
fn short_mission_name(level: &LevelDefinition) -> String {
    let name = tr_owned(&level.name);
    match name.split_once(". ") {
        Some((number, rest)) if number.len() <= 3 => rest.to_string(),
        _ => name,
    }
}

/// The briefing beside the route: the story, the objective and map, the campaign difficulty and the start.
fn spawn_mission_briefing(
    panel: &mut ChildSpawnerCommands,
    fonts: &UiFonts,
    menu: &MainMenu,
    campaign: usize,
    mission: usize,
    preview: Option<Handle<Image>>,
) {
    let chosen = &menu.campaigns[campaign];
    let level = chosen.mission_at(mission, menu.campaign_difficulty);
    panel.spawn(fonts.text(
        tr_fmt(
            "{} campaign  ·  mission {} of {}",
            &[
                &tr_owned(&chosen.title),
                &(mission + 1),
                &chosen.missions.len(),
            ],
        ),
        15.0,
        TEXT_DIM,
    ));
    panel.spawn(fonts.heading(tr_owned(&level.name), 30.0, ACCENT));
    beside_preview(panel, preview, LARGE_PREVIEW, |column| {
        column.spawn(fonts.text(tr_owned(&level.briefing), 17.0, TEXT_MAIN));
        column.spawn(Node {
            height: Val::Px(4.0),
            ..default()
        });
        spawn_level_facts(column, fonts, &level);
    });
    let difficulty = format!(
        "{}: {}",
        tr("Campaign difficulty"),
        tr_owned(&format!("{:?}", menu.campaign_difficulty))
    );
    spawn_command_button(
        panel,
        fonts,
        &difficulty,
        MenuCommand::CycleCampaignDifficulty,
    );
    if chosen.unlocked(mission, &menu.completed_missions) {
        spawn_command_button(
            panel,
            fonts,
            tr("START MISSION"),
            MenuCommand::StartCampaignMission,
        );
    } else {
        panel.spawn(fonts.text(
            tr_fmt(
                "Locked: win {} first.",
                &[&tr_owned(&chosen.missions[mission - 1].name)],
            ),
            16.0,
            TEXT_DIM,
        ));
    }
}

fn full_width() -> f32 {
    NAV_WIDTH + LIST_WIDTH + DETAIL_WIDTH + 2.0 * BODY_GAP
}

fn spawn_section_column(body: &mut ChildSpawnerCommands, fonts: &UiFonts, group: MenuGroup) {
    body.spawn(Node {
        width: Val::Px(NAV_WIDTH),
        flex_shrink: 0.0,
        flex_direction: FlexDirection::Column,
        row_gap: Val::Px(8.0),
        ..default()
    })
    .with_children(|column| {
        spawn_command_button(column, fonts, tr("< Back"), MenuCommand::BackToHub);
        column.spawn(fonts.heading(group.title(), 24.0, ACCENT));
        let sections = group.sections();
        for &section in sections.iter().filter(|_| sections.len() > 1) {
            column
                .spawn((
                    SectionButton(section),
                    Hovered::default(),
                    Node {
                        min_height: Val::Px(46.0),
                        ..button_node(Val::Percent(100.0))
                    },
                    BackgroundColor(BUTTON_IDLE),
                    BorderColor::all(FRAME),
                ))
                .with_children(|button| {
                    button.spawn(fonts.heading(section.label(), 19.0, TEXT_MAIN));
                })
                .observe(open_section);
        }
    });
}

fn open_section(
    click: On<PointerClick>,
    buttons: Query<&SectionButton>,
    mut menu: ResMut<MainMenu>,
) {
    if let Ok(button) = buttons.get(click.entity) {
        menu.selection = menu.first_in(button.0);
        menu.message = None;
        menu.needs_rebuild = true;
    }
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
    spawn_command_button_sized(parent, fonts, label, command, Val::Percent(100.0));
}

fn spawn_command_button_sized(
    parent: &mut ChildSpawnerCommands,
    fonts: &UiFonts,
    label: &str,
    command: MenuCommand,
    width: Val,
) {
    let (height, size, background) = if command.is_primary() {
        (44.0, 20.0, PRIMARY_IDLE)
    } else {
        (32.0, 17.0, BUTTON_IDLE)
    };
    parent
        .spawn((
            command,
            Hovered::default(),
            Node {
                min_height: Val::Px(height),
                margin: UiRect::top(Val::Px(if command.is_primary() { 6.0 } else { 0.0 })),
                ..button_node(width)
            },
            BackgroundColor(background),
            BorderColor::all(FRAME),
        ))
        .with_children(|button| {
            button.spawn(fonts.heading(label, size, TEXT_MAIN));
        })
        .observe(run_command);
}

fn spawn_list_button(
    parent: &mut ChildSpawnerCommands,
    fonts: &UiFonts,
    label: &str,
    selection: Selection,
) {
    spawn_list_button_with_badge(parent, fonts, label, selection, None);
}

/// A list entry with a short coloured note on its right, such as a won mission.
fn spawn_list_button_with_badge(
    parent: &mut ChildSpawnerCommands,
    fonts: &UiFonts,
    label: &str,
    selection: Selection,
    badge: Option<(&str, Color)>,
) {
    parent
        .spawn((
            ListButton(selection),
            Hovered::default(),
            Node {
                justify_content: JustifyContent::SpaceBetween,
                ..button_node(Val::Percent(100.0))
            },
            BackgroundColor(BUTTON_IDLE),
            BorderColor::all(FRAME),
        ))
        .with_children(|button| {
            button.spawn(fonts.heading(label, 17.0, TEXT_MAIN));
            if let Some((badge, color)) = badge {
                button.spawn(fonts.heading(badge, 13.0, color));
            }
        })
        .observe(select_entry);
}

fn rebuild_level_list(commands: &mut Commands, list: Entity, fonts: &UiFonts, menu: &MainMenu) {
    commands.entity(list).despawn_related::<Children>();
    commands
        .entity(list)
        .with_children(|list| match menu.section() {
            MenuSection::Campaign => spawn_operations_map(list, fonts, menu),
            MenuSection::Custom => {
                let mut any = false;
                for (index, entry) in menu.entries.iter().enumerate() {
                    if matches!(entry.source, LevelSource::Custom(_)) {
                        any = true;
                        spawn_list_button(
                            list,
                            fonts,
                            &tr_owned(&entry.level.name),
                            Selection::Level(index),
                        );
                    }
                }
                if !any {
                    list.spawn(fonts.text(
                        tr("No custom levels yet - make one in the battle editor."),
                        15.0,
                        TEXT_DIM,
                    ));
                }
            }
            MenuSection::Replays => {
                for (index, replay) in menu.replays.iter().enumerate().take(REPLAYS_LISTED) {
                    let label = format!(
                        "{}  ({})",
                        tr_owned(&replay.title),
                        minutes(replay.recorded_secs)
                    );
                    spawn_list_button(list, fonts, &label, Selection::Replay(index));
                }
                if menu.replays.is_empty() {
                    list.spawn(fonts.text(tr("Every battle is recorded here."), 15.0, TEXT_DIM));
                }
            }
            MenuSection::Quick
            | MenuSection::Generated
            | MenuSection::Random
            | MenuSection::Editor
            | MenuSection::Network => {}
        });
}

/// The facts of a level as label, value and colour, for a two-column table.
fn level_facts(level: &LevelDefinition) -> Vec<(&'static str, String, Color)> {
    let mut facts = vec![
        (tr("Objective"), level.objective.describe(), ACCENT),
        (
            tr("Map"),
            tr_fmt(
                "{}, {}, {}, {} x {} cells",
                &[
                    &tr_owned(&format!("{:?}", level.landscape)),
                    &tr_owned(&format!("{:?}", level.season)),
                    &tr_owned(&format!("{:?}", level.weather)),
                    &level.map_size.cells(),
                    &level.map_size.cells(),
                ],
            ),
            TEXT_MAIN,
        ),
        (
            tr(Team::Player.name_in(level.player_faction)),
            format!("{}  ·  ${}", level.player.site.name(), level.player.credits),
            Team::Player.ui_color(),
        ),
    ];
    for (team, enemy) in level.opponents() {
        facts.push((
            tr(team.name_in(level.faction_of(team))),
            format!(
                "{}  ·  {}  ·  ${}",
                enemy.force.site.name(),
                tr_owned(&format!("{:?}", enemy.difficulty)),
                enemy.force.credits
            ),
            team.ui_color(),
        ));
    }
    if level.enemies.len() > 1 {
        facts.push((
            tr("Opponents"),
            alliance_name(level.effective_alliance()).to_string(),
            TEXT_MAIN,
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
    facts.push((tr("Units"), units, TEXT_DIM));
    facts
}

fn spawn_level_facts(column: &mut ChildSpawnerCommands, fonts: &UiFonts, level: &LevelDefinition) {
    for (label, value, color) in level_facts(level) {
        column
            .spawn(Node {
                column_gap: Val::Px(10.0),
                ..default()
            })
            .with_children(|row| {
                row.spawn((
                    fonts.heading(label, 14.0, TEXT_DIM),
                    Node {
                        width: Val::Px(FACT_LABEL_WIDTH),
                        flex_shrink: 0.0,
                        ..default()
                    },
                ));
                row.spawn((
                    fonts.text(value, 16.0, color),
                    Node {
                        flex_shrink: 1.0,
                        ..default()
                    },
                ));
            });
    }
}

const ADDRESS_CHARACTERS: &str = "0123456789.:-abcdefghijklmnopqrstuvwxyz";
const LONGEST_ADDRESS: usize = 64;

impl MainMenu {
    fn section(&self) -> MenuSection {
        match self.selection {
            Selection::Level(index) => match self.entries[index].source {
                LevelSource::Custom(_) => MenuSection::Custom,
                LevelSource::Campaign(_) | LevelSource::Skirmish => MenuSection::Campaign,
            },
            Selection::Mission { .. } => MenuSection::Campaign,
            Selection::Editor => MenuSection::Editor,
            Selection::Network => MenuSection::Network,
            Selection::Replay(_) => MenuSection::Replays,
            Selection::Quick => MenuSection::Quick,
            Selection::Generated => MenuSection::Generated,
            Selection::Random => MenuSection::Random,
            Selection::Nothing(section) => section,
        }
    }

    /// What a section shows first: its first entry, or its page.
    fn first_in(&self, section: MenuSection) -> Selection {
        let first_level = |custom: bool| {
            self.entries
                .iter()
                .position(|entry| matches!(entry.source, LevelSource::Custom(_)) == custom)
                .map(Selection::Level)
        };
        let first = match section {
            MenuSection::Campaign => Some(self.campaign_page(0)),
            MenuSection::Custom => first_level(true),
            MenuSection::Quick => Some(Selection::Quick),
            MenuSection::Generated => Some(Selection::Generated),
            MenuSection::Random => Some(Selection::Random),
            MenuSection::Editor => Some(Selection::Editor),
            MenuSection::Network => Some(Selection::Network),
            MenuSection::Replays => (!self.replays.is_empty()).then_some(Selection::Replay(0)),
        };
        first.unwrap_or(Selection::Nothing(section))
    }

    /// A campaign's page opens on the mission to play next, or its first once it is finished.
    fn campaign_page(&self, campaign: usize) -> Selection {
        let mission = self
            .campaigns
            .get(campaign)
            .and_then(|c| c.next_to_play(&self.completed_missions))
            .unwrap_or(0);
        Selection::Mission { campaign, mission }
    }

    /// The campaign played last and its next mission, while it has one.
    fn campaign_to_continue(&self, last: &str) -> Option<(usize, usize)> {
        let campaign = self.campaigns.iter().position(|c| c.key == last)?;
        let mission = self.campaigns[campaign].next_to_play(&self.completed_missions)?;
        Some((campaign, mission))
    }

    fn open_group(&mut self, group: MenuGroup) {
        self.screen = Screen::Group(group);
        self.selection = self.first_in(group.sections()[0]);
        self.message = None;
        self.needs_new_screen = true;
    }

    fn show_hub(&mut self) {
        self.screen = Screen::Hub;
        self.message = None;
        self.needs_new_screen = true;
    }

    /// Opens a section by its English name, for capture runs.
    pub fn open_section_named(&mut self, name: &str) {
        let section = match name {
            "campaign" => MenuSection::Campaign,
            "custom" => MenuSection::Custom,
            "quick" => MenuSection::Quick,
            "generator" => MenuSection::Generated,
            "random" => MenuSection::Random,
            "editor" => MenuSection::Editor,
            "network" => MenuSection::Network,
            "replays" => MenuSection::Replays,
            _ => return,
        };
        let group = MenuGroup::holding(section);
        if self.screen != Screen::Group(group) {
            self.screen = Screen::Group(group);
            self.needs_new_screen = true;
        }
        if self.section() != section || matches!(self.selection, Selection::Nothing(_)) {
            self.selection = self.first_in(section);
            self.needs_rebuild = true;
        }
    }

    /// The level whose map the page shows, if it has one.
    fn previewed_level(&self) -> Option<LevelDefinition> {
        match self.selection {
            Selection::Level(index) => Some(self.entries[index].level.clone()),
            Selection::Mission { campaign, mission } => {
                Some(self.campaigns[campaign].mission_at(mission, self.campaign_difficulty))
            }
            Selection::Random => Some(self.random_level()),
            Selection::Generated => Some(self.generated_mission()),
            Selection::Editor => Some(self.options.to_level()),
            Selection::Network
            | Selection::Replay(_)
            | Selection::Quick
            | Selection::Nothing(_) => None,
        }
    }

    fn update_preview(&mut self, images: &mut Assets<Image>) -> Option<Handle<Image>> {
        let level = self.previewed_level()?;
        let current = self
            .preview
            .as_ref()
            .filter(|(shown, _)| *shown == level)
            .map(|(_, image)| image.clone());
        Some(current.unwrap_or_else(|| {
            let image = images.add(map_preview(&level));
            self.preview = Some((level, image.clone()));
            image
        }))
    }

    fn random_level(&self) -> LevelDefinition {
        random_battle::generate(&self.random, self.random_seed)
    }

    fn generated_mission(&self) -> LevelDefinition {
        generate_mission(self.random_seed, self.quick_faction, self.quick_difficulty)
    }

    /// The battle the editor-like pages describe right now.
    fn level_on_page(&self) -> LevelDefinition {
        match self.selection {
            Selection::Random => self.random_level(),
            Selection::Generated => self.generated_mission(),
            _ => self.options.to_level(),
        }
    }

    pub fn show_network_page(&mut self) {
        if self.screen != Screen::Group(MenuGroup::Multiplayer) {
            self.screen = Screen::Group(MenuGroup::Multiplayer);
            self.needs_new_screen = true;
        }
        if self.selection != Selection::Network {
            self.selection = Selection::Network;
            self.needs_rebuild = true;
        }
    }
}

/// Typing on the network battle page: the host's address before connecting, a chat line afterwards.
pub fn type_on_network_page(
    menu: Option<Res<MainMenu>>,
    mut typed: MessageReader<KeyboardInput>,
    mut settings: ResMut<GameSettings>,
    mut lobby: ResMut<NetLobby>,
    mut address_fields: Query<&mut Text, (With<JoinAddressText>, Without<ChatDraftText>)>,
    mut draft_fields: Query<&mut Text, With<ChatDraftText>>,
) {
    let on_network_page = menu.is_some_and(|menu| menu.selection == Selection::Network);
    if !on_network_page {
        typed.clear();
        return;
    }
    let chatting = lobby.has_partner();
    for key in typed.read().filter(|key| key.state.is_pressed()) {
        match (&key.logical_key, chatting) {
            (Key::Enter, true) => lobby.say(),
            (Key::Backspace, true) => {
                lobby.draft.pop();
            }
            (Key::Backspace, false) => {
                settings.join_address.pop();
            }
            (Key::Space, true) => lobby.draft.push(' '),
            (Key::Character(text), true) => lobby.draft.push_str(text),
            (Key::Character(text), false) => {
                for character in text.to_lowercase().chars() {
                    let fits = settings.join_address.len() < LONGEST_ADDRESS;
                    if ADDRESS_CHARACTERS.contains(character) && fits {
                        settings.join_address.push(character);
                    }
                }
            }
            _ => {}
        }
    }
    for mut field in &mut address_fields {
        field.set_if_neq(Text(settings.join_address.clone()));
    }
    for mut field in &mut draft_fields {
        field.set_if_neq(Text(format!("{}_", lobby.draft)));
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

fn spawn_editor_fields(
    panel: &mut ChildSpawnerCommands,
    fonts: &UiFonts,
    options: &SkirmishOptions,
    fields: &[EditorField],
) {
    let rows = fields
        .iter()
        .map(|&field| (field.label(), field.value(options), EditorOption(field)));
    spawn_option_grid(panel, fonts, rows);
}

/// Labelled values in two columns; clicking a value cycles it forward, right-clicking back.
fn spawn_option_grid<M: Component>(
    panel: &mut ChildSpawnerCommands,
    fonts: &UiFonts,
    rows: impl IntoIterator<Item = (&'static str, String, M)>,
) {
    panel
        .spawn(Node {
            flex_wrap: FlexWrap::Wrap,
            column_gap: Val::Px(24.0),
            row_gap: Val::Px(6.0),
            ..default()
        })
        .with_children(|grid| {
            for (label, value, marker) in rows {
                grid.spawn(Node {
                    width: Val::Px(EDITOR_FIELD_WIDTH),
                    justify_content: JustifyContent::SpaceBetween,
                    align_items: AlignItems::Center,
                    ..default()
                })
                .with_children(|row| {
                    row.spawn(fonts.text(label, 17.0, TEXT_MAIN));
                    row.spawn((
                        marker,
                        Hovered::default(),
                        button_node(Val::Px(EDITOR_VALUE_WIDTH)),
                        BackgroundColor(BUTTON_IDLE),
                        BorderColor::all(FRAME),
                    ))
                    .with_children(|button| {
                        button.spawn(fonts.heading(value, 16.0, ACCENT));
                    })
                    .observe(change_option);
                });
            }
        });
}

fn alliance_name(alliance: Alliance) -> &'static str {
    match alliance {
        Alliance::CoalitionAgainstSyndicate => tr("Allied"),
        Alliance::FreeForAll => tr("Free-for-all"),
        Alliance::TwoAgainstTwo => tr("Two against two"),
    }
}

/// One click to a battle: a side and a difficulty, everything else rolled anew each time.
fn spawn_quick_battle(panel: &mut ChildSpawnerCommands, fonts: &UiFonts, menu: &MainMenu) {
    panel.spawn(fonts.heading(tr("Quick battle"), 30.0, ACCENT));
    panel.spawn(fonts.text(
        tr("Pick a side and a difficulty. One opponent on a standard map; the land, season, weather and corners are rolled every time."),
        16.0,
        TEXT_DIM,
    ));
    spawn_side_and_difficulty(panel, fonts, menu);
    panel.spawn(Node {
        height: Val::Px(12.0),
        ..default()
    });
    spawn_command_button(
        panel,
        fonts,
        tr("START QUICK BATTLE"),
        MenuCommand::StartQuickBattle,
    );
}

/// The side and difficulty choices the quick battle and the mission generator share.
fn spawn_side_and_difficulty(panel: &mut ChildSpawnerCommands, fonts: &UiFonts, menu: &MainMenu) {
    let factions = [
        (
            FactionChoice::Fixed(Faction::Coalition),
            tr(Faction::Coalition.title()),
        ),
        (
            FactionChoice::Fixed(Faction::Syndicate),
            tr(Faction::Syndicate.title()),
        ),
        (FactionChoice::Random, tr("RANDOM")),
    ];
    spawn_choice_row(
        panel,
        fonts,
        tr("Your side"),
        factions.into_iter().map(|(choice, label)| {
            (
                label.to_string(),
                MenuCommand::QuickFaction(choice),
                choice == menu.quick_faction,
            )
        }),
    );
    spawn_choice_row(
        panel,
        fonts,
        tr("Difficulty"),
        Difficulty::ALL.into_iter().map(|difficulty| {
            (
                tr_owned(&format!("{difficulty:?}")),
                MenuCommand::QuickDifficulty(difficulty),
                difficulty == menu.quick_difficulty,
            )
        }),
    );
}

/// A whole mission rolled from one seed: the battle, an objective, an operation name and its briefing.
fn spawn_generated_mission(
    panel: &mut ChildSpawnerCommands,
    fonts: &UiFonts,
    menu: &MainMenu,
    preview: Option<Handle<Image>>,
) {
    panel.spawn(fonts.heading(tr("Mission generator"), 30.0, ACCENT));
    spawn_side_and_difficulty(panel, fonts, menu);
    let level = menu.generated_mission();
    beside_preview(panel, preview, SMALL_PREVIEW, |column| {
        column.spawn(fonts.heading(&level.name, 20.0, TEXT_MAIN));
        column.spawn(fonts.text(&level.briefing, 15.0, TEXT_DIM));
        spawn_level_facts(column, fonts, &level);
    });
    spawn_command_button(
        panel,
        fonts,
        tr("START MISSION"),
        MenuCommand::StartEditorBattle,
    );
    spawn_command_button(
        panel,
        fonts,
        tr("NEW MISSION"),
        MenuCommand::RerollRandomBattle,
    );
    spawn_command_button(
        panel,
        fonts,
        tr("SAVE AS CUSTOM LEVEL"),
        MenuCommand::SaveEditorLevel,
    );
}

/// A labelled row of buttons of which one is chosen, outlined in the accent colour.
fn spawn_choice_row(
    panel: &mut ChildSpawnerCommands,
    fonts: &UiFonts,
    label: &str,
    choices: impl Iterator<Item = (String, MenuCommand, bool)>,
) {
    panel.spawn(fonts.heading(label, 17.0, TEXT_MAIN));
    panel
        .spawn(Node {
            column_gap: Val::Px(8.0),
            margin: UiRect::bottom(Val::Px(6.0)),
            ..default()
        })
        .with_children(|row| {
            for (text, command, chosen) in choices {
                row.spawn((
                    command,
                    Hovered::default(),
                    Node {
                        min_width: Val::Px(120.0),
                        min_height: Val::Px(40.0),
                        justify_content: JustifyContent::Center,
                        ..button_node(Val::Auto)
                    },
                    BackgroundColor(BUTTON_IDLE),
                    BorderColor::all(if chosen { ACCENT } else { FRAME }),
                ))
                .with_children(|button| {
                    button.spawn(fonts.heading(
                        text,
                        17.0,
                        if chosen { ACCENT } else { TEXT_MAIN },
                    ));
                })
                .observe(run_command);
            }
        });
}

fn spawn_random_battle_details(
    panel: &mut ChildSpawnerCommands,
    fonts: &UiFonts,
    menu: &MainMenu,
    preview: Option<Handle<Image>>,
) {
    panel.spawn(fonts.heading(tr("Random battle"), 30.0, ACCENT));
    panel.spawn(fonts.text(
        tr("Choose the opponents, map size and credits; the landscape, season, weather, corners, alliances and the forests, rocks and crystals are rolled. New map rolls them again."),
        15.0,
        TEXT_DIM,
    ));
    let rows = RandomField::ALL.map(|field| {
        (
            field.label(),
            field.value(&menu.random),
            RandomOption(field),
        )
    });
    spawn_option_grid(panel, fonts, rows);
    let level = menu.random_level();
    beside_preview(panel, preview, SMALL_PREVIEW, |column| {
        column.spawn(fonts.heading(&level.name, 20.0, TEXT_MAIN));
        spawn_level_facts(column, fonts, &level);
    });
    spawn_command_button(
        panel,
        fonts,
        tr("START BATTLE"),
        MenuCommand::StartEditorBattle,
    );
    spawn_command_button(panel, fonts, tr("NEW MAP"), MenuCommand::RerollRandomBattle);
    spawn_command_button(
        panel,
        fonts,
        tr("SAVE AS CUSTOM LEVEL"),
        MenuCommand::SaveEditorLevel,
    );
    spawn_command_button(panel, fonts, tr("EDIT MAP"), MenuCommand::EditMap);
}

fn spawn_network_details(
    panel: &mut ChildSpawnerCommands,
    fonts: &UiFonts,
    join_address: &str,
    lobby: &NetLobby,
    options: &SkirmishOptions,
) {
    panel.spawn(fonts.heading(tr("Network battle"), 30.0, ACCENT));
    panel.spawn(fonts.text(
        tr("Two to four players on their own computers. The host leads the faction chosen in the battle editor's settings and can still change them; the others take the computer corners. Anyone who joins once every seat is taken watches the whole battle. The host starts when everyone is ready. Everyone needs the same version of the game, and the host's port 47300 must be reachable."),
        15.0,
        TEXT_DIM,
    ));
    if !lobby.is_busy() {
        spawn_command_button(
            panel,
            fonts,
            tr("HOST A BATTLE"),
            MenuCommand::HostNetworkBattle,
        );
        spawn_address_row(panel, fonts, join_address);
        spawn_command_button(panel, fonts, tr("JOIN"), MenuCommand::JoinNetworkBattle);
        spawn_command_button(
            panel,
            fonts,
            tr("OPEN THIS ROOM ON THE SERVER"),
            MenuCommand::OpenServerRoom,
        );
        panel.spawn(fonts.text(
            tr("On a dedicated server, type ws://server/room/NAME above: the first to open the room picks the battle from the battle editor's settings, and everyone else joins the same address."),
            14.0,
            TEXT_DIM,
        ));
    }
    panel.spawn((NetworkStatusText, fonts.text("", 16.0, GOOD)));
    if lobby.is_hosting() {
        let while_hosting: Vec<EditorField> = EDITOR_FIELDS
            .into_iter()
            .filter(|&field| field != EditorField::NetworkPlayers)
            .collect();
        spawn_editor_fields(panel, fonts, options, &while_hosting);
    } else if let Some(level) = lobby.offered_level() {
        spawn_level_facts(panel, fonts, level);
    }
    if lobby.has_partner() {
        spawn_chat(panel, fonts, lobby);
    }
    if lobby.can_accept_offer() {
        spawn_command_button(
            panel,
            fonts,
            tr("I'M READY"),
            MenuCommand::AcceptNetworkOffer,
        );
    }
    if lobby.can_start() {
        spawn_command_button(
            panel,
            fonts,
            tr("START THE BATTLE"),
            MenuCommand::StartNetworkBattle,
        );
    }
    if lobby.is_busy() {
        spawn_command_button(panel, fonts, tr("LEAVE"), MenuCommand::CancelNetworkBattle);
    }
}

fn spawn_text_field(
    panel: &mut ChildSpawnerCommands,
    fonts: &UiFonts,
    label: &str,
    value: String,
    marker: impl Component,
) {
    panel
        .spawn(Node {
            justify_content: JustifyContent::SpaceBetween,
            align_items: AlignItems::Center,
            ..default()
        })
        .with_children(|row| {
            row.spawn(fonts.text(label, 18.0, TEXT_MAIN));
            row.spawn((
                button_node(Val::Px(260.0)),
                BackgroundColor(BUTTON_IDLE),
                BorderColor::all(ACCENT),
            ))
            .with_children(|field| {
                field.spawn((marker, fonts.heading(value, 16.0, ACCENT)));
            });
        });
}

fn spawn_address_row(panel: &mut ChildSpawnerCommands, fonts: &UiFonts, join_address: &str) {
    let label = tr("Host address (type it)");
    spawn_text_field(
        panel,
        fonts,
        label,
        join_address.to_string(),
        JoinAddressText,
    );
}

fn spawn_chat(panel: &mut ChildSpawnerCommands, fonts: &UiFonts, lobby: &NetLobby) {
    for line in lobby.chat() {
        let who = if line.mine {
            tr("You")
        } else {
            tr(line.team.name())
        };
        let color = line.team.ui_color();
        panel.spawn(fonts.text(format!("{who}: {}", line.text), 15.0, color));
    }
    let label = tr("Message (Enter sends)");
    spawn_text_field(
        panel,
        fonts,
        label,
        format!("{}_", lobby.draft),
        ChatDraftText,
    );
}

fn rebuild_details(
    commands: &mut Commands,
    panel: Entity,
    fonts: &UiFonts,
    menu: &MainMenu,
    preview: Option<Handle<Image>>,
    join_address: &str,
    lobby: &NetLobby,
) {
    commands.entity(panel).despawn_related::<Children>();
    commands
        .entity(panel)
        .with_children(|panel| match menu.selection {
            Selection::Mission { campaign, mission } => {
                spawn_mission_briefing(panel, fonts, menu, campaign, mission, preview.clone())
            }
            Selection::Level(index) => {
                let level = &menu.entries[index].level;
                panel.spawn(fonts.heading(tr_owned(&level.name), 30.0, ACCENT));
                beside_preview(panel, preview, LARGE_PREVIEW, |column| {
                    column.spawn(fonts.text(tr_owned(&level.briefing), 17.0, TEXT_MAIN));
                    column.spawn(Node {
                        height: Val::Px(4.0),
                        ..default()
                    });
                    spawn_level_facts(column, fonts, level);
                });
                spawn_command_button(panel, fonts, tr("START MISSION"), MenuCommand::StartSelected);
                if matches!(menu.entries[index].source, LevelSource::Custom(_)) {
                    spawn_command_button(panel, fonts, tr("EDIT MAP"), MenuCommand::EditMap);
                    spawn_command_button(
                        panel,
                        fonts,
                        tr("ADD TO MY CAMPAIGN"),
                        MenuCommand::AddToMyCampaign,
                    );
                }
            }
            Selection::Replay(index) => {
                let replay = &menu.replays[index];
                panel.spawn(fonts.heading(tr_owned(&replay.title), 30.0, ACCENT));
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
            Selection::Network => {
                spawn_network_details(panel, fonts, join_address, lobby, &menu.options)
            }
            Selection::Random => spawn_random_battle_details(panel, fonts, menu, preview),
            Selection::Quick => spawn_quick_battle(panel, fonts, menu),
            Selection::Generated => spawn_generated_mission(panel, fonts, menu, preview),
            Selection::Nothing(section) => {
                panel.spawn(fonts.heading(section.label(), 30.0, ACCENT));
                let hint = match section {
                    MenuSection::Custom => tr(
                        "Levels saved from the battle editor or the map editor appear here. Open the battle editor, set up a battle and save it, or edit its map.",
                    ),
                    _ => tr("Finish or leave a battle and its replay appears here."),
                };
                panel.spawn(fonts.text(hint, 16.0, TEXT_DIM));
            }
            Selection::Editor => {
                panel.spawn(fonts.heading(tr("Battle editor"), 30.0, ACCENT));
                panel.spawn(fonts.text(
                    tr("Left click: next value, right click: previous."),
                    15.0,
                    TEXT_DIM,
                ));
                spawn_editor_fields(panel, fonts, &menu.options, &EDITOR_FIELDS);
                beside_preview(panel, preview, SMALL_PREVIEW, |column| {
                    column.spawn(fonts.text(
                        menu.options.to_level().objective.describe(),
                        16.0,
                        TEXT_DIM,
                    ));
                });
                spawn_command_button(panel, fonts, tr("START BATTLE"), MenuCommand::StartEditorBattle);
                spawn_command_button(
                    panel,
                    fonts,
                    tr("SAVE AS CUSTOM LEVEL"),
                    MenuCommand::SaveEditorLevel,
                );
                spawn_command_button(panel, fonts, tr("EDIT MAP"), MenuCommand::EditMap);
            }
        });
    if let Some(message) = &menu.message {
        commands.entity(panel).with_children(|panel| {
            panel.spawn(fonts.text(message.clone(), 15.0, GOOD));
        });
    }
}

/// Lays `content` out in a column with the map picture to its right.
fn beside_preview(
    panel: &mut ChildSpawnerCommands,
    preview: Option<Handle<Image>>,
    size: f32,
    content: impl FnOnce(&mut ChildSpawnerCommands),
) {
    panel
        .spawn(Node {
            column_gap: Val::Px(16.0),
            align_items: AlignItems::FlexStart,
            ..default()
        })
        .with_children(|row| {
            row.spawn(Node {
                flex_direction: FlexDirection::Column,
                flex_grow: 1.0,
                row_gap: Val::Px(8.0),
                ..default()
            })
            .with_children(content);
            if let Some(image) = preview {
                row.spawn((
                    ImageNode::new(image),
                    Node {
                        width: Val::Px(size),
                        height: Val::Px(size),
                        flex_shrink: 0.0,
                        border: UiRect::all(Val::Px(1.0)),
                        ..default()
                    },
                    BorderColor::all(FRAME),
                ));
            }
        });
}

/// A campaign mission won on this computer is remembered and marked in the main menu.
pub fn remember_won_missions(
    outcome: Res<GameOutcome>,
    run: Option<Res<CampaignRun>>,
    library: Res<CampaignLibrary>,
    session: Option<Res<NetSession>>,
    mut settings: ResMut<GameSettings>,
) {
    if !outcome.is_changed() || *outcome != GameOutcome::Victory || session.is_some() {
        return;
    }
    let Some(run) = run else { return };
    let Some(campaign) = find_campaign(&library.0, &run.key) else {
        return;
    };
    let key = campaign.progress_key(run.mission);
    if !settings.completed_missions.contains(&key) {
        settings.completed_missions.push(key);
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
    random_fields: Query<&RandomOption>,
    mut menu: ResMut<MainMenu>,
    mut lobby: ResMut<NetLobby>,
) {
    let forward = click.button != PointerButton::Secondary;
    if let Ok(field) = random_fields.get(click.entity) {
        field.0.change(&mut menu.random, forward);
        menu.message = None;
        menu.needs_rebuild = true;
        return;
    }
    let Ok(field) = fields.get(click.entity) else {
        return;
    };
    field.0.change(&mut menu.options, forward);
    if lobby.is_hosting() {
        let players = menu.options.network_players;
        lobby.reoffer(network_level(&menu.options, players));
    }
    menu.message = None;
    menu.needs_rebuild = true;
}

fn start_level(
    commands: &mut Commands,
    next_state: &mut NextState<GameState>,
    level: LevelDefinition,
) {
    commands.remove_resource::<CampaignRun>();
    commands.insert_resource(ActiveLevel(level));
    next_state.set(GameState::Playing);
}

fn start_campaign_mission(
    commands: &mut Commands,
    next_state: &mut NextState<GameState>,
    settings: &mut GameSettings,
    menu: &MainMenu,
    campaign: usize,
    mission: usize,
) {
    let chosen = &menu.campaigns[campaign];
    if !chosen.unlocked(mission, &menu.completed_missions) {
        return;
    }
    start_level(
        commands,
        next_state,
        chosen.mission_at(mission, menu.campaign_difficulty),
    );
    commands.insert_resource(CampaignRun {
        key: chosen.key.clone(),
        mission,
        difficulty: menu.campaign_difficulty,
    });
    settings.last_campaign = chosen.key.clone();
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
    mut settings_menu: ResMut<MenuState>,
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
        MenuCommand::QuickFaction(faction) => {
            menu.quick_faction = faction;
            menu.needs_rebuild = true;
        }
        MenuCommand::QuickDifficulty(difficulty) => {
            menu.quick_difficulty = difficulty;
            menu.needs_rebuild = true;
        }
        MenuCommand::StartQuickBattle => {
            let options = RandomBattleOptions {
                faction: menu.quick_faction,
                difficulty: DifficultyChoice::Fixed(menu.quick_difficulty),
                ..default()
            };
            let level = random_battle::generate(&options, rand::rng().random());
            start_level(&mut commands, &mut next_state, level);
        }
        MenuCommand::StartEditorBattle => {
            start_level(&mut commands, &mut next_state, menu.level_on_page())
        }
        MenuCommand::RerollRandomBattle => {
            menu.random_seed = rand::rng().random();
            menu.message = None;
            menu.needs_rebuild = true;
        }
        MenuCommand::SaveEditorLevel => {
            let level = menu.level_on_page();
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
        MenuCommand::EditMap => {
            let (level, file) = match menu.selection {
                Selection::Level(index) => {
                    let entry = &menu.entries[index];
                    let file = match &entry.source {
                        LevelSource::Custom(path) => Some(path.clone()),
                        LevelSource::Campaign(_) | LevelSource::Skirmish => None,
                    };
                    (entry.level.clone(), file)
                }
                _ => (menu.level_on_page(), None),
            };
            open_map_editor(&mut commands, &mut next_state, level, file);
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
        MenuCommand::StartCampaignMission => {
            if let Selection::Mission { campaign, mission } = menu.selection {
                start_campaign_mission(
                    &mut commands,
                    &mut next_state,
                    &mut settings,
                    &menu,
                    campaign,
                    mission,
                );
            }
        }
        MenuCommand::CycleCampaignDifficulty => {
            menu.campaign_difficulty = cycle(&Difficulty::ALL, menu.campaign_difficulty, true);
            settings.campaign_difficulty = menu.campaign_difficulty;
            menu.needs_rebuild = true;
        }
        MenuCommand::AddToMyCampaign => {
            let Selection::Level(index) = menu.selection else {
                return;
            };
            let entry = &menu.entries[index];
            let LevelSource::Custom(path) = &entry.source else {
                return;
            };
            let added =
                add_to_my_campaign(&custom_levels_directory(), path, entry.level.player_faction);
            menu.message = Some(match added {
                Ok(count) => tr_fmt(
                    "Added - My campaign now has {} missions under Campaigns.",
                    &[&count],
                ),
                Err(error) => tr_fmt("Could not add the level: {}", &[&error]),
            });
            let mut campaigns = built_in_campaigns();
            campaigns.extend(custom_campaigns(&custom_levels_directory()));
            menu.campaigns = campaigns;
            menu.needs_rebuild = true;
        }
        MenuCommand::ContinueCampaign => {
            if let Some((campaign, mission)) = menu.campaign_to_continue(&settings.last_campaign) {
                start_campaign_mission(
                    &mut commands,
                    &mut next_state,
                    &mut settings,
                    &menu,
                    campaign,
                    mission,
                );
            }
        }
        MenuCommand::OpenSettings => open_settings(&mut settings_menu),
        MenuCommand::BackToHub => menu.show_hub(),
        MenuCommand::DownloadUpdate => download_update(&mut updates),
        MenuCommand::HostNetworkBattle => {
            let players = menu.options.network_players;
            lobby.host(network_level(&menu.options, players), players);
        }
        MenuCommand::JoinNetworkBattle => lobby.join(&settings.join_address),
        MenuCommand::OpenServerRoom => {
            let players = menu.options.network_players;
            lobby.open_room(
                &settings.join_address,
                network_level(&menu.options, players),
                players,
            );
        }
        MenuCommand::AcceptNetworkOffer => lobby.accept_offer(),
        MenuCommand::StartNetworkBattle => lobby.start_battle(),
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
    mut images: ResMut<Assets<Image>>,
    lists: Query<Entity, With<LevelList>>,
    mut list_columns: Query<&mut Node, With<ListColumn>>,
    mut sections: Query<(
        &SectionButton,
        &Hovered,
        &mut BackgroundColor,
        &mut BorderColor,
    )>,
    panels: Query<Entity, With<DetailPanel>>,
    mut hub_tiles: Query<
        (&Hovered, &mut BackgroundColor, &mut BorderColor),
        (
            With<HubTile>,
            Without<SectionButton>,
            Without<ListButton>,
            Without<MenuCommand>,
            Without<EditorOption>,
        ),
    >,
    mut buttons: Query<
        (
            &Hovered,
            &mut BackgroundColor,
            Option<&ListButton>,
            Option<&MenuCommand>,
        ),
        (
            Or<(With<ListButton>, With<MenuCommand>, With<EditorOption>)>,
            Without<SectionButton>,
        ),
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
        let shown = if menu.section().has_list() {
            Display::Flex
        } else {
            Display::None
        };
        for mut column in &mut list_columns {
            set_field(&mut column, |node| &mut node.display, shown);
        }
        rebuild_level_list(&mut commands, list, &fonts, &menu);
        let preview = menu.update_preview(&mut images);
        rebuild_details(
            &mut commands,
            panel,
            &fonts,
            &menu,
            preview,
            &settings.join_address,
            &lobby,
        );
    }
    for (hovered, mut background, mut border) in &mut hub_tiles {
        background.set_if_neq(BackgroundColor(if hovered.get() {
            BUTTON_HOVERED
        } else {
            BUTTON_IDLE
        }));
        border.set_if_neq(BorderColor::all(if hovered.get() { ACCENT } else { FRAME }));
    }
    let open = menu.section();
    for (section, hovered, mut background, mut border) in &mut sections {
        let selected = section.0 == open;
        background.set_if_neq(BackgroundColor(if selected {
            BUTTON_SELECTED
        } else if hovered.get() {
            BUTTON_HOVERED
        } else {
            BUTTON_IDLE
        }));
        border.set_if_neq(BorderColor::all(if selected { ACCENT } else { FRAME }));
    }
    for (hovered, mut background, list_button, command) in &mut buttons {
        let selected = list_button.is_some_and(|button| button.0 == menu.selection);
        let primary = command.is_some_and(|command| command.is_primary());
        background.set_if_neq(BackgroundColor(match (selected, primary, hovered.get()) {
            (true, ..) => BUTTON_SELECTED,
            (false, true, true) => PRIMARY_HOVERED,
            (false, true, false) => PRIMARY_IDLE,
            (false, false, true) => BUTTON_HOVERED,
            (false, false, false) => BUTTON_IDLE,
        }));
    }
}
