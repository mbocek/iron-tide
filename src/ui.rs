use crate::abilities::MINE_COST;
use crate::i18n::{tr, tr_fmt, tr_owned};
use crate::key_bindings::{Action, KeyBindings};
use crate::local_view::LocalView;
use crate::match_stats::{MatchStats, TeamTally};
use crate::player_commands::LocalTeam;
use crate::player_commands::{CommandIssuer, PlayerCommand};
use crate::replay::{ReplayPlayback, list_replays, load_replay, replay_directory};
use crate::research::Upgrade;
use crate::structure_tools::spawn_tool_buttons;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::text::{FontSize, FontSource};

use crate::catalog::{BuildingKind, Faction, HARVESTER_CAPACITY, Team, UnitKind};
use crate::components::*;
use crate::control_groups::spawn_group_bar;
use crate::economy::Economies;
use crate::levels::ActiveLevel;
use crate::match_flow::{GameState, MatchClock};
use crate::menu::{MenuState, open_menu_from_button, spawn_menu};
use crate::minimap::{Minimap, jump_camera_on_drag, jump_camera_on_press};
use crate::network_hud::{spawn_chat_line, spawn_network_hud};
use crate::notifications::Notifications;
use crate::outcome::{GameOutcome, objective_status};
use crate::perf_overlay::spawn_performance_overlay;
use crate::production::{
    BuildItem, EnqueueRefusal, Lane, Placement, Production, owned_building_kinds,
};
use crate::queue_bar::spawn_queue_bar;
use crate::settings::GameSettings;
use crate::sound::{Sound, SoundCue};

pub const SIDEBAR_WIDTH: f32 = 300.0;

pub(crate) const PANEL: Color = Color::srgb(0.065, 0.075, 0.085);
const PANEL_RAISED: Color = Color::srgb(0.10, 0.115, 0.13);
pub(crate) const FRAME: Color = Color::srgb(0.22, 0.25, 0.28);
pub(crate) const FRAME_BRIGHT: Color = Color::srgb(0.38, 0.43, 0.47);
const TILE: Color = Color::srgb(0.13, 0.15, 0.17);
const TILE_HOVERED: Color = Color::srgb(0.18, 0.21, 0.24);
const TILE_READY: Color = Color::srgb(0.12, 0.30, 0.16);
const SHADE: Color = Color::srgba(0.0, 0.0, 0.0, 0.62);
pub(crate) const TEXT_MAIN: Color = Color::srgb(0.90, 0.92, 0.93);
pub(crate) const TEXT_DIM: Color = Color::srgb(0.52, 0.56, 0.60);
pub(crate) const ACCENT: Color = Color::srgb(0.96, 0.74, 0.26);
pub(crate) const GOOD: Color = Color::srgb(0.35, 0.86, 0.42);
const WARNING: Color = Color::srgb(0.95, 0.78, 0.22);
const DANGER: Color = Color::srgb(1.0, 0.33, 0.28);
const TILE_WIDTH: f32 = 88.0;
const TILE_HEIGHT: f32 = 76.0;
const TILE_ICON_SIZE: f32 = 42.0;
const MINIMAP_SIZE: f32 = 236.0;
const POWER_BAR_SCALE: i32 = 400;
const SHIFT_CLICK_BATCH: u32 = 5;

#[derive(Resource)]
pub struct UiFonts {
    regular: Handle<Font>,
    bold: Handle<Font>,
}

#[derive(Resource)]
pub struct ActiveTab(Lane);

#[derive(Component)]
pub struct BuildTile(BuildItem);

#[derive(Component)]
pub struct TileShade(BuildItem);

#[derive(Component)]
pub struct TileBadge(BuildItem);

#[derive(Component)]
pub struct TileIcon(BuildItem);

#[derive(Component)]
pub struct TabButton(Lane);

#[derive(Component)]
pub struct CreditsText;

#[derive(Component)]
pub struct PowerText;

#[derive(Component)]
pub struct PowerBarFill;

#[derive(Component)]
pub struct PowerBarDemand;

#[derive(Component)]
pub struct InfoIcon;

#[derive(Component)]
pub struct InfoTitle;

#[derive(Component)]
pub struct InfoBody;

#[derive(Component)]
pub struct InfoHealthFill;

/// Icons of the selected kinds with their counts, when several units are selected.
#[derive(Component)]
pub struct InfoRoster;

#[derive(Component)]
pub struct NotificationLine(usize);

#[derive(Component)]
pub struct ControlsPanel;

#[derive(Component)]
pub struct ControlsHint;

#[derive(Component)]
pub struct ObjectiveText;

#[derive(Component)]
pub struct OutcomeOverlay;

#[derive(Component)]
pub struct OutcomeTitle;

#[derive(Component)]
pub struct OutcomeStats;

impl UiFonts {
    pub(crate) fn text(&self, text: impl Into<String>, size: f32, color: Color) -> impl Bundle {
        (
            Text::new(text),
            TextFont {
                font: FontSource::Handle(self.regular.clone()),
                font_size: FontSize::Px(size),
                ..default()
            },
            TextColor(color),
        )
    }

    pub(crate) fn heading(&self, text: impl Into<String>, size: f32, color: Color) -> impl Bundle {
        (
            Text::new(text),
            TextFont {
                font: FontSource::Handle(self.bold.clone()),
                font_size: FontSize::Px(size),
                ..default()
            },
            TextColor(color),
        )
    }
}

pub(crate) fn icon_path(item: BuildItem) -> String {
    let model_id = match item {
        BuildItem::Structure(kind) => kind.spec().model_id,
        BuildItem::Unit(kind) => kind.spec().model_id,
        BuildItem::Research(upgrade) => upgrade.spec().icon_id,
    };
    format!("icons/{model_id}.png")
}

fn lane_items(lane: Lane, faction: Faction) -> Vec<BuildItem> {
    let items = match lane {
        Lane::Structures => BuildingKind::CONSTRUCTIBLE
            .map(BuildItem::Structure)
            .to_vec(),
        Lane::Infantry => UnitKind::INFANTRY.map(BuildItem::Unit).to_vec(),
        Lane::Vehicles => UnitKind::VEHICLES.map(BuildItem::Unit).to_vec(),
        Lane::Research => Upgrade::ALL.map(BuildItem::Research).to_vec(),
    };
    items
        .into_iter()
        .filter(|item| item.available_to(faction))
        .collect()
}

fn lane_title(lane: Lane) -> &'static str {
    match lane {
        Lane::Structures => tr("BASE"),
        Lane::Infantry => tr("INFANTRY"),
        Lane::Vehicles => tr("VEHICLES"),
        Lane::Research => tr("RESEARCH"),
    }
}

fn framed(width: Val, height: Val) -> Node {
    Node {
        width,
        height,
        border: UiRect::all(Val::Px(1.0)),
        ..default()
    }
}

pub fn load_fonts(mut commands: Commands, asset_server: Res<AssetServer>) {
    commands.insert_resource(UiFonts {
        regular: asset_server.load("fonts/Rajdhani-SemiBold.ttf"),
        bold: asset_server.load("fonts/Rajdhani-Bold.ttf"),
    });
}

pub fn build_interface(
    local: Res<LocalTeam>,
    view: LocalView,
    production: Res<Production>,
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    fonts: Res<UiFonts>,
    minimap: Res<Minimap>,
) {
    let fonts = &*fonts;

    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                right: Val::Px(0.0),
                top: Val::Px(0.0),
                bottom: Val::Px(0.0),
                width: Val::Px(SIDEBAR_WIDTH),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(10.0)),
                row_gap: Val::Px(8.0),
                border: UiRect::left(Val::Px(2.0)),
                ..default()
            },
            BackgroundColor(PANEL),
            BorderColor::all(FRAME_BRIGHT),
        ))
        .with_children(|sidebar| {
            let faction = production.faction_of(local.0);
            let title = if view.is_spectating() {
                tr("SPECTATOR")
            } else {
                tr(faction.title())
            };
            spawn_header(sidebar, fonts, title, local.0);
            spawn_minimap(sidebar, &minimap);
            if !view.is_spectating() {
                spawn_resources(sidebar, fonts);
                spawn_objective(sidebar, fonts);
                spawn_tabs(sidebar, fonts);
                spawn_build_grid(sidebar, fonts, &asset_server, faction);
            }
            spawn_info_panel(sidebar, fonts);
        });

    spawn_notification_feed(&mut commands, fonts);
    spawn_queue_bar(&mut commands, fonts);
    spawn_group_bar(&mut commands, fonts);
    spawn_performance_overlay(&mut commands, fonts);
    spawn_network_hud(&mut commands, fonts);
    spawn_chat_line(&mut commands, fonts);
    spawn_menu(&mut commands, fonts);
    spawn_controls_help(&mut commands, fonts);
    spawn_outcome_overlay(&mut commands, fonts);
    commands.insert_resource(ActiveTab(Lane::Structures));
}

fn spawn_header(
    sidebar: &mut ChildSpawnerCommands,
    fonts: &UiFonts,
    title: &'static str,
    team: Team,
) {
    sidebar
        .spawn(Node {
            flex_direction: FlexDirection::Row,
            justify_content: JustifyContent::SpaceBetween,
            align_items: AlignItems::Center,
            ..default()
        })
        .with_children(|header| {
            header.spawn(fonts.heading("IRON TIDE", 30.0, ACCENT));
            header.spawn(fonts.heading(title, 16.0, team.ui_color()));
            header
                .spawn((
                    Node {
                        padding: UiRect::axes(Val::Px(8.0), Val::Px(2.0)),
                        border: UiRect::all(Val::Px(1.0)),
                        ..default()
                    },
                    BackgroundColor(TILE),
                    BorderColor::all(FRAME_BRIGHT),
                ))
                .with_children(|button| {
                    button.spawn(fonts.heading("MENU", 14.0, TEXT_MAIN));
                })
                .observe(|_: On<PointerClick>, menu: ResMut<MenuState>| {
                    open_menu_from_button(menu)
                });
        });
}

fn spawn_minimap(sidebar: &mut ChildSpawnerCommands, minimap: &Minimap) {
    sidebar
        .spawn((
            Node {
                align_self: AlignSelf::Center,
                ..framed(Val::Px(MINIMAP_SIZE + 2.0), Val::Px(MINIMAP_SIZE + 2.0))
            },
            BorderColor::all(FRAME_BRIGHT),
            BackgroundColor(Color::BLACK),
        ))
        .with_children(|frame| {
            frame
                .spawn((
                    ImageNode::new(minimap.image.clone()),
                    Node {
                        width: Val::Px(MINIMAP_SIZE),
                        height: Val::Px(MINIMAP_SIZE),
                        ..default()
                    },
                ))
                .observe(jump_camera_on_press)
                .observe(jump_camera_on_drag);
        });
}

fn spawn_resources(sidebar: &mut ChildSpawnerCommands, fonts: &UiFonts) {
    sidebar
        .spawn((
            Node {
                flex_direction: FlexDirection::Column,
                padding: UiRect::axes(Val::Px(8.0), Val::Px(6.0)),
                row_gap: Val::Px(4.0),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(PANEL_RAISED),
            BorderColor::all(FRAME),
        ))
        .with_children(|panel| {
            panel
                .spawn(Node {
                    justify_content: JustifyContent::SpaceBetween,
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(10.0),
                    ..default()
                })
                .with_children(|row| {
                    row.spawn((CreditsText, fonts.heading("$ 0", 26.0, ACCENT)));
                    spawn_tool_buttons(row, fonts);
                });
            panel
                .spawn(Node {
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(8.0),
                    ..default()
                })
                .with_children(|row| {
                    row.spawn((
                        PowerText,
                        fonts.text(tr_fmt("POWER {} used of {}", &[&0, &0]), 14.0, TEXT_MAIN),
                    ));
                    row.spawn((
                        Node {
                            flex_grow: 1.0,
                            height: Val::Px(8.0),
                            ..default()
                        },
                        BackgroundColor(Color::srgb(0.04, 0.05, 0.05)),
                    ))
                    .with_children(|bar| {
                        bar.spawn((
                            PowerBarFill,
                            Node {
                                position_type: PositionType::Absolute,
                                height: Val::Percent(100.0),
                                ..default()
                            },
                            BackgroundColor(GOOD),
                        ));
                        bar.spawn((
                            PowerBarDemand,
                            Node {
                                position_type: PositionType::Absolute,
                                width: Val::Px(2.0),
                                height: Val::Px(12.0),
                                top: Val::Px(-2.0),
                                ..default()
                            },
                            BackgroundColor(TEXT_MAIN),
                        ));
                    });
                });
        });
}

fn spawn_tabs(sidebar: &mut ChildSpawnerCommands, fonts: &UiFonts) {
    sidebar
        .spawn(Node {
            column_gap: Val::Px(4.0),
            ..default()
        })
        .with_children(|tabs| {
            for lane in Lane::ALL {
                tabs.spawn((
                    Hovered::default(),
                    TabButton(lane),
                    Node {
                        flex_grow: 1.0,
                        height: Val::Px(30.0),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        border: UiRect::all(Val::Px(1.0)),
                        ..default()
                    },
                    BackgroundColor(TILE),
                    BorderColor::all(FRAME),
                ))
                .with_children(|tab| {
                    tab.spawn(fonts.heading(lane_title(lane), 15.0, TEXT_MAIN));
                })
                .observe(select_tab);
            }
        });
}

fn spawn_build_grid(
    sidebar: &mut ChildSpawnerCommands,
    fonts: &UiFonts,
    asset_server: &AssetServer,
    faction: Faction,
) {
    sidebar
        .spawn(Node {
            flex_wrap: FlexWrap::Wrap,
            column_gap: Val::Px(6.0),
            row_gap: Val::Px(6.0),
            flex_grow: 1.0,
            align_content: AlignContent::FlexStart,
            ..default()
        })
        .with_children(|grid| {
            for item in Lane::ALL
                .into_iter()
                .flat_map(|lane| lane_items(lane, faction))
            {
                spawn_build_tile(grid, fonts, asset_server, item);
            }
        });
}

fn spawn_build_tile(
    grid: &mut ChildSpawnerCommands,
    fonts: &UiFonts,
    asset_server: &AssetServer,
    item: BuildItem,
) {
    grid.spawn((
        Hovered::default(),
        BuildTile(item),
        Node {
            width: Val::Px(TILE_WIDTH),
            height: Val::Px(TILE_HEIGHT),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            padding: UiRect::all(Val::Px(3.0)),
            border: UiRect::all(Val::Px(1.0)),
            ..default()
        },
        BackgroundColor(TILE),
        BorderColor::all(FRAME),
    ))
    .with_children(|tile| {
        tile.spawn((
            TileIcon(item),
            ImageNode::new(asset_server.load(icon_path(item))),
            Node {
                width: Val::Px(TILE_ICON_SIZE),
                height: Val::Px(TILE_ICON_SIZE),
                ..default()
            },
        ));
        tile.spawn(fonts.heading(item.name(), 12.0, TEXT_MAIN));
        tile.spawn(fonts.heading(format!("${}", item.cost()), 12.0, ACCENT));
        tile.spawn((
            TileShade(item),
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                bottom: Val::Px(0.0),
                width: Val::Percent(100.0),
                height: Val::Percent(0.0),
                ..default()
            },
            BackgroundColor(SHADE),
        ));
        tile.spawn((
            TileBadge(item),
            fonts.heading("", 12.0, GOOD),
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(2.0),
                right: Val::Px(4.0),
                ..default()
            },
        ));
    })
    .observe(click_build_tile);
}

fn spawn_objective(sidebar: &mut ChildSpawnerCommands, fonts: &UiFonts) {
    sidebar
        .spawn((
            Node {
                flex_direction: FlexDirection::Column,
                padding: UiRect::axes(Val::Px(8.0), Val::Px(5.0)),
                row_gap: Val::Px(1.0),
                border: UiRect::left(Val::Px(3.0)),
                ..default()
            },
            BackgroundColor(PANEL_RAISED),
            BorderColor::all(ACCENT),
        ))
        .with_children(|panel| {
            panel.spawn(fonts.heading(tr("OBJECTIVE"), 12.0, TEXT_DIM));
            panel.spawn((ObjectiveText, fonts.text("", 14.0, TEXT_MAIN)));
        });
}

fn spawn_info_panel(sidebar: &mut ChildSpawnerCommands, fonts: &UiFonts) {
    sidebar
        .spawn((
            Node {
                min_height: Val::Px(104.0),
                padding: UiRect::all(Val::Px(8.0)),
                column_gap: Val::Px(8.0),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(PANEL_RAISED),
            BorderColor::all(FRAME),
        ))
        .with_children(|panel| {
            panel.spawn((
                InfoIcon,
                ImageNode::default(),
                Node {
                    width: Val::Px(64.0),
                    height: Val::Px(64.0),
                    flex_shrink: 0.0,
                    display: Display::None,
                    ..default()
                },
            ));
            panel
                .spawn(Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(3.0),
                    flex_grow: 1.0,
                    ..default()
                })
                .with_children(|column| {
                    column.spawn((InfoTitle, fonts.heading("", 18.0, TEXT_MAIN)));
                    column.spawn((InfoBody, fonts.text("", 14.0, TEXT_DIM)));
                    column.spawn((
                        InfoRoster,
                        Node {
                            flex_wrap: FlexWrap::Wrap,
                            column_gap: Val::Px(4.0),
                            row_gap: Val::Px(4.0),
                            ..default()
                        },
                    ));
                    column
                        .spawn((
                            Node {
                                width: Val::Percent(100.0),
                                height: Val::Px(6.0),
                                ..default()
                            },
                            BackgroundColor(Color::srgb(0.04, 0.05, 0.05)),
                        ))
                        .with_children(|bar| {
                            bar.spawn((
                                InfoHealthFill,
                                Node {
                                    width: Val::Percent(0.0),
                                    height: Val::Percent(100.0),
                                    ..default()
                                },
                                BackgroundColor(GOOD),
                            ));
                        });
                });
        });
}

fn spawn_roster_chip(
    row: &mut ChildSpawnerCommands,
    fonts: &UiFonts,
    asset_server: &AssetServer,
    kind: UnitKind,
    count: usize,
) {
    row.spawn((
        Node {
            align_items: AlignItems::Center,
            padding: UiRect::right(Val::Px(5.0)),
            border: UiRect::all(Val::Px(1.0)),
            ..default()
        },
        BackgroundColor(TILE),
        BorderColor::all(FRAME),
    ))
    .with_children(|chip| {
        chip.spawn((
            ImageNode::new(asset_server.load(format!("icons/{}.png", kind.spec().model_id))),
            Node {
                width: Val::Px(30.0),
                height: Val::Px(30.0),
                ..default()
            },
        ));
        chip.spawn(fonts.heading(format!("{count}"), 15.0, TEXT_MAIN));
    });
}

fn spawn_notification_feed(commands: &mut Commands, fonts: &UiFonts) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(14.0),
                left: Val::Px(0.0),
                right: Val::Px(SIDEBAR_WIDTH),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: Val::Px(2.0),
                ..default()
            },
            Pickable::IGNORE,
        ))
        .with_children(|feed| {
            for index in 0..4 {
                feed.spawn((
                    NotificationLine(index),
                    fonts.heading("", 22.0, ACCENT),
                    TextShadow::default(),
                    Pickable::IGNORE,
                ));
            }
        });
}

fn spawn_controls_help(commands: &mut Commands, fonts: &UiFonts) {
    commands.spawn((
        ControlsHint,
        fonts.text("", 15.0, TEXT_DIM),
        TextShadow::default(),
        Pickable::IGNORE,
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(12.0),
            bottom: Val::Px(8.0),
            ..default()
        },
    ));
    commands.spawn((
        ControlsPanel,
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(12.0),
            bottom: Val::Px(34.0),
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(Val::Px(10.0)),
            row_gap: Val::Px(2.0),
            border: UiRect::all(Val::Px(1.0)),
            ..default()
        },
        BackgroundColor(PANEL.with_alpha(0.92)),
        BorderColor::all(FRAME_BRIGHT),
        Visibility::Hidden,
    ));
}

fn controls_overview(keys: &KeyBindings) -> Vec<(&'static str, String)> {
    let key = |action| keys.label(action);
    vec![
        (
            tr("Select"),
            tr_owned("left click / drag a box (Shift adds)"),
        ),
        (tr("Orders"), tr_owned("right click: move, attack, harvest")),
        (tr("Attack-move"), tr_owned("Ctrl + right click")),
        (tr("Patrol"), tr_owned("Alt + right click")),
        (tr("Queue orders"), tr_owned("Shift + right click")),
        (
            tr("Rally point"),
            tr_owned("select barracks/factory, right click"),
        ),
        (
            tr("Repair / sell"),
            tr_fmt(
                "{} / {} (or the buttons), then click a structure",
                &[&key(Action::RepairTool), &key(Action::SellTool)],
            ),
        ),
        (
            tr("Scroll"),
            tr_fmt(
                "{}{}{}{}, arrows, screen edge, middle-drag",
                &[
                    &key(Action::ScrollUp),
                    &key(Action::ScrollLeft),
                    &key(Action::ScrollDown),
                    &key(Action::ScrollRight),
                ],
            ),
        ),
        (tr("Fast scroll"), tr_owned("hold Shift")),
        (tr("Minimap"), tr_owned("click or drag to jump")),
        (
            tr("Zoom / rotate"),
            tr_fmt(
                "mouse wheel / {}, {} ({} resets)",
                &[
                    &key(Action::RotateLeft),
                    &key(Action::RotateRight),
                    &key(Action::ResetRotation),
                ],
            ),
        ),
        (tr("Home"), tr_fmt("{} or Home", &[&key(Action::JumpHome)])),
        (
            tr("Stop / deselect"),
            format!("{} / Esc", key(Action::Stop)),
        ),
        (
            tr("All of one kind"),
            tr_owned("double-click or Ctrl+click a unit"),
        ),
        (
            tr("Last attack"),
            tr_fmt("{} jumps there", &[&key(Action::JumpToAlert)]),
        ),
        (
            tr("Lay a mine"),
            tr_fmt(
                "{} with engineers selected, ${} each",
                &[&key(Action::LayMine), &MINE_COST],
            ),
        ),
        (
            tr("Groups"),
            tr_owned("Ctrl+1-9 save, 1-9 select, twice = jump"),
        ),
        (tr("Add a group"), "Shift+1-9".to_string()),
        (
            tr("Graphics quality"),
            tr_fmt("{}: Low / Medium / High", &[&key(Action::GraphicsQuality)]),
        ),
        (
            tr("Performance"),
            tr_fmt(
                "{} overlay: FPS, CPU, GPU",
                &[&key(Action::PerformanceOverlay)],
            ),
        ),
        (
            tr("Save / load"),
            tr_fmt(
                "{} quick save, {} quick load",
                &[&key(Action::QuickSave), &key(Action::QuickLoad)],
            ),
        ),
        (tr("Menu & settings"), tr_owned("F10 or Esc")),
        (
            tr("Build menu"),
            tr_owned("left click builds (Shift = 5x), right click cancels"),
        ),
        (
            tr("Queue bar"),
            tr_owned("left click places a ready structure, right click cancels"),
        ),
    ]
}

/// The overview names the player's own keys, so it is rewritten whenever the settings change.
pub fn fill_controls_help(
    mut commands: Commands,
    fonts: Option<Res<UiFonts>>,
    settings: Res<GameSettings>,
    panels: Query<(Entity, Ref<ControlsPanel>)>,
    mut hints: Query<&mut Text, With<ControlsHint>>,
) {
    let Some(fonts) = fonts else {
        return;
    };
    for (panel, added) in &panels {
        if !settings.is_changed() && !added.is_added() {
            continue;
        }
        for mut hint in &mut hints {
            hint.0 = tr_fmt(
                "{}  controls",
                &[&settings.keys.label(Action::ControlsHelp)],
            );
        }
        commands.entity(panel).despawn_related::<Children>();
        commands.entity(panel).with_children(|panel| {
            panel.spawn(fonts.heading(tr("CONTROLS"), 18.0, ACCENT));
            for (action, keys) in controls_overview(&settings.keys) {
                panel
                    .spawn(Node {
                        column_gap: Val::Px(10.0),
                        ..default()
                    })
                    .with_children(|row| {
                        row.spawn((
                            fonts.heading(action, 15.0, TEXT_MAIN),
                            Node {
                                width: Val::Px(110.0),
                                ..default()
                            },
                        ));
                        row.spawn(fonts.text(keys, 15.0, TEXT_DIM));
                    });
            }
        });
    }
}

fn spawn_outcome_overlay(commands: &mut Commands, fonts: &UiFonts) {
    commands
        .spawn((
            OutcomeOverlay,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Px(0.0),
                bottom: Val::Px(0.0),
                right: Val::Px(SIDEBAR_WIDTH),
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                row_gap: Val::Px(14.0),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.55)),
            Visibility::Hidden,
        ))
        .with_children(|overlay| {
            overlay.spawn((
                OutcomeTitle,
                fonts.heading("", 96.0, ACCENT),
                TextShadow::default(),
            ));
            overlay.spawn((
                OutcomeStats,
                Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(4.0),
                    padding: UiRect::axes(Val::Px(24.0), Val::Px(14.0)),
                    border: UiRect::all(Val::Px(1.0)),
                    min_width: Val::Px(460.0),
                    ..default()
                },
                BackgroundColor(PANEL.with_alpha(0.9)),
                BorderColor::all(FRAME_BRIGHT),
            ));
            overlay
                .spawn(Node {
                    column_gap: Val::Px(12.0),
                    ..default()
                })
                .with_children(|buttons| {
                    spawn_outcome_button(
                        buttons,
                        fonts,
                        tr("WATCH REPLAY"),
                        OutcomeAction::WatchReplay,
                    );
                    spawn_outcome_button(buttons, fonts, tr("MAIN MENU"), OutcomeAction::MainMenu);
                });
            overlay.spawn(fonts.text(
                tr("F10 opens the menu to restart the mission."),
                18.0,
                TEXT_DIM,
            ));
        });
}

#[derive(Component, Clone, Copy)]
pub enum OutcomeAction {
    WatchReplay,
    MainMenu,
}

fn spawn_outcome_button(
    parent: &mut ChildSpawnerCommands,
    fonts: &UiFonts,
    label: &str,
    action: OutcomeAction,
) {
    parent
        .spawn((
            action,
            Hovered::default(),
            Node {
                padding: UiRect::axes(Val::Px(18.0), Val::Px(8.0)),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(TILE),
            BorderColor::all(FRAME_BRIGHT),
        ))
        .with_children(|button| {
            button.spawn(fonts.heading(label, 18.0, TEXT_MAIN));
        })
        .observe(run_outcome_action);
}

fn run_outcome_action(
    click: On<PointerClick>,
    mut commands: Commands,
    actions: Query<&OutcomeAction>,
    mut notifications: ResMut<Notifications>,
    mut next_state: ResMut<NextState<GameState>>,
) {
    let Ok(&action) = actions.get(click.entity) else {
        return;
    };
    match action {
        OutcomeAction::MainMenu => next_state.set(GameState::MainMenu),
        OutcomeAction::WatchReplay => {
            let newest = list_replays(&replay_directory())
                .into_iter()
                .next()
                .and_then(|summary| load_replay(&summary.path).ok());
            match newest {
                Some(replay) => {
                    commands.insert_resource(ActiveLevel(replay.level.clone()));
                    commands.insert_resource(ReplayPlayback::new(replay));
                    next_state.set(GameState::Replay);
                }
                None => notifications.push(tr("The replay could not be found")),
            }
        }
    }
}

/// The rows of the end-of-match table: what the player did against what all opponents did together.
fn outcome_rows(
    stats: &MatchStats,
    economies: &Economies,
    team: Team,
) -> Vec<(&'static str, String, String)> {
    let player = *stats.of(team);
    let mut opponents = TeamTally::default();
    let mut opponents_harvested = 0;
    for team in Team::OPPONENTS {
        opponents.add(stats.of(team));
        opponents_harvested += economies.of(team).harvested;
    }
    let count = |value: u32| value.to_string();
    let amount = |value: f32| format!("{:.0}", value);
    vec![
        (
            tr("Units trained"),
            count(player.units_trained),
            count(opponents.units_trained),
        ),
        (
            tr("Units lost"),
            count(player.units_lost),
            count(opponents.units_lost),
        ),
        (
            tr("Structures built"),
            count(player.structures_built),
            count(opponents.structures_built),
        ),
        (
            tr("Structures lost"),
            count(player.structures_lost),
            count(opponents.structures_lost),
        ),
        (
            tr("Enemies destroyed"),
            count(player.enemies_destroyed),
            count(opponents.enemies_destroyed),
        ),
        (
            tr("Damage dealt"),
            amount(player.damage_dealt),
            amount(opponents.damage_dealt),
        ),
        (
            tr("Credits harvested"),
            format!("${}", economies.of(team).harvested),
            format!("${opponents_harvested}"),
        ),
    ]
}

fn spawn_outcome_table(
    table: &mut ChildSpawnerCommands,
    fonts: &UiFonts,
    rows: Vec<(&'static str, String, String)>,
    match_secs: f32,
    team: Team,
) {
    let row = |table: &mut ChildSpawnerCommands, cells: [(String, Color); 3]| {
        table
            .spawn(Node {
                column_gap: Val::Px(12.0),
                ..default()
            })
            .with_children(|line| {
                for (index, (text, color)) in cells.into_iter().enumerate() {
                    let width = if index == 0 { 220.0 } else { 100.0 };
                    line.spawn(Node {
                        width: Val::Px(width),
                        justify_content: if index == 0 {
                            JustifyContent::FlexStart
                        } else {
                            JustifyContent::FlexEnd
                        },
                        ..default()
                    })
                    .with_children(|cell| {
                        cell.spawn(fonts.text(text, 18.0, color));
                    });
                }
            });
    };
    let minutes = match_secs as u32 / 60;
    let seconds = match_secs as u32 % 60;
    let time = format!("{minutes}:{seconds:02}");
    table.spawn(fonts.heading(tr_fmt("MATCH TIME  {}", &[&time]), 18.0, ACCENT));
    row(
        table,
        [
            (String::new(), TEXT_DIM),
            (tr("You").into(), team.ui_color()),
            (tr("Enemies").into(), Team::Crimson.ui_color()),
        ],
    );
    for (label, player, opponents) in rows {
        row(
            table,
            [
                (label.into(), TEXT_DIM),
                (player, TEXT_MAIN),
                (opponents, TEXT_MAIN),
            ],
        );
    }
}

/// Writes a field only when it differs: any write marks the whole component changed, and changed
/// interface nodes make Bevy measure text and lay out the interface again.
pub fn set_field<C, V: PartialEq>(
    component: &mut Mut<C>,
    field: impl FnOnce(&mut C) -> &mut V,
    value: V,
) {
    let slot = field(component.bypass_change_detection());
    if *slot != value {
        *slot = value;
        component.set_changed();
    }
}

pub fn toggle_controls_help(
    keys: Res<ButtonInput<KeyCode>>,
    settings: Res<GameSettings>,
    mut panels: Query<&mut Visibility, With<ControlsPanel>>,
) {
    if !settings.keys.just_pressed(&keys, Action::ControlsHelp) {
        return;
    }
    for mut visibility in &mut panels {
        *visibility = match *visibility {
            Visibility::Hidden => Visibility::Visible,
            _ => Visibility::Hidden,
        };
    }
}

fn select_tab(click: On<PointerClick>, tabs: Query<&TabButton>, mut active: ResMut<ActiveTab>) {
    if let Ok(tab) = tabs.get(click.entity) {
        active.0 = tab.0;
    }
}

fn click_build_tile(
    mut click: On<PointerClick>,
    mut cues: MessageWriter<SoundCue>,
    mut issuer: CommandIssuer,
    keys: Res<ButtonInput<KeyCode>>,
    tiles: Query<&BuildTile>,
    production: Res<Production>,
    mut placement: ResMut<Placement>,
    mut notifications: ResMut<Notifications>,
    outcome: Res<GameOutcome>,
    buildings: Query<(&Building, &Team)>,
) {
    let Ok(&BuildTile(item)) = tiles.get(click.entity) else {
        return;
    };
    click.propagate(false);
    if *outcome != GameOutcome::Ongoing {
        return;
    }
    let team = issuer.team();
    if click.button == PointerButton::Secondary {
        issuer.issue(PlayerCommand::Cancel { item });
        if let BuildItem::Structure(kind) = item
            && placement.active == Some(kind)
        {
            placement.active = None;
        }
        return;
    }
    if click.button != PointerButton::Primary {
        return;
    }
    let owned = owned_building_kinds(&buildings, team);
    if let BuildItem::Structure(kind) = item
        && production.lane(team, item.lane()).ready == Some(kind)
    {
        placement.active = Some(kind);
        return;
    }
    if let BuildItem::Structure(kind) = item
        && kind.is_wall()
    {
        let refusal = if !production.allows(team, item) {
            Some(EnqueueRefusal::NotInMission.message())
        } else {
            item.missing_requirement(&owned)
                .map(|missing| tr_fmt("Requires {}", &[&tr(missing.spec().name)]))
        };
        match refusal {
            Some(message) => {
                notifications.push(message);
                cues.write(SoundCue::interface(Sound::UiError));
            }
            None => placement.active = Some(kind),
        }
        return;
    }
    if let Some(missing) = item.missing_requirement(&owned) {
        notifications.push(tr_fmt("Requires {}", &[&tr(missing.spec().name)]));
        return;
    }
    let batch = if keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]) {
        SHIFT_CLICK_BATCH
    } else {
        1
    };
    issuer.issue(PlayerCommand::Enqueue {
        item,
        copies: batch,
    });
}

enum TileState {
    NotInMission,
    Locked,
    Ready,
    Building { fraction: f32, queued: usize },
    Queued(usize),
    Unaffordable,
    Available,
    Researched,
}

fn tile_state(
    item: BuildItem,
    production: &Production,
    owned: &[BuildingKind],
    credits: i32,
    team: Team,
) -> TileState {
    if !production.allows(team, item) {
        return TileState::NotInMission;
    }
    if let BuildItem::Research(upgrade) = item
        && production.has_researched(team, upgrade)
    {
        return TileState::Researched;
    }
    if item.missing_requirement(owned).is_some() {
        return TileState::Locked;
    }
    let lane = production.lane(team, item.lane());
    if let BuildItem::Structure(kind) = item
        && lane.ready == Some(kind)
    {
        return TileState::Ready;
    }
    let queued = lane.queue.iter().filter(|&&q| q == item).count();
    if lane.queue.front() == Some(&item) {
        return TileState::Building {
            fraction: lane.progress_fraction(),
            queued,
        };
    }
    if queued > 0 {
        return TileState::Queued(queued);
    }
    if credits < item.cost() {
        return TileState::Unaffordable;
    }
    TileState::Available
}

pub fn refresh_build_tiles(
    local: Res<LocalTeam>,
    time: Res<Time>,
    active: Res<ActiveTab>,
    production: Res<Production>,
    economies: Res<Economies>,
    buildings: Query<(&Building, &Team)>,
    mut tiles: Query<(
        &BuildTile,
        &Hovered,
        &mut Node,
        &mut BackgroundColor,
        &mut BorderColor,
    )>,
    mut shades: Query<(&TileShade, &mut Node), Without<BuildTile>>,
    mut badges: Query<(&TileBadge, &mut Text, &mut TextColor)>,
    mut icons: Query<(&TileIcon, &mut ImageNode)>,
    mut tabs: Query<
        (&TabButton, &Hovered, &mut BackgroundColor, &mut BorderColor),
        Without<BuildTile>,
    >,
) {
    let owned = owned_building_kinds(&buildings, local.0);
    let credits = economies.of(local.0).credits;
    let team = local.0;
    let state_of = |item| tile_state(item, &production, &owned, credits, team);
    let pulse = (time.elapsed_secs() * 5.0).sin() * 0.5 + 0.5;

    for (tile, hovered, mut node, mut background, mut border) in &mut tiles {
        let display = if tile.0.lane() == active.0 {
            Display::Flex
        } else {
            Display::None
        };
        set_field(&mut node, |node| &mut node.display, display);
        let hovered = hovered.get();
        let ready = matches!(state_of(tile.0), TileState::Ready);
        background.set_if_neq(BackgroundColor(match (ready, hovered) {
            (true, _) => TILE_READY,
            (false, true) => TILE_HOVERED,
            (false, false) => TILE,
        }));
        border.set_if_neq(BorderColor::all(if ready {
            GOOD.mix(&TEXT_MAIN, pulse * 0.5)
        } else if hovered {
            ACCENT
        } else {
            FRAME
        }));
    }
    for (shade, mut node) in &mut shades {
        let covered = match state_of(shade.0) {
            TileState::NotInMission | TileState::Locked | TileState::Unaffordable => 1.0,
            TileState::Building { fraction, .. } => 1.0 - fraction,
            _ => 0.0,
        };
        set_field(
            &mut node,
            |node| &mut node.height,
            Val::Percent(covered * 100.0),
        );
    }
    for (badge, mut text, mut color) in &mut badges {
        let (label, tint) = match state_of(badge.0) {
            TileState::Ready => (tr("READY").to_string(), GOOD),
            TileState::Building { fraction, queued } if queued > 1 => (
                format!("{}%  x{queued}", (fraction * 100.0) as u32),
                TEXT_MAIN,
            ),
            TileState::Building { fraction, .. } => {
                (format!("{}%", (fraction * 100.0) as u32), TEXT_MAIN)
            }
            TileState::Queued(count) => (format!("x{count}"), TEXT_MAIN),
            TileState::Locked => (tr("LOCKED").to_string(), TEXT_DIM),
            TileState::NotInMission => (tr("N/A").to_string(), TEXT_DIM),
            TileState::Researched => (tr("RESEARCHED").to_string(), GOOD),
            TileState::Unaffordable | TileState::Available => (String::new(), TEXT_MAIN),
        };
        text.set_if_neq(Text(label));
        color.set_if_neq(TextColor(tint));
    }
    for (icon, mut image) in &mut icons {
        let locked = matches!(
            state_of(icon.0),
            TileState::Locked | TileState::NotInMission
        );
        image.color = if locked {
            Color::srgba(0.5, 0.5, 0.5, 0.5)
        } else {
            Color::WHITE
        };
    }
    for (tab, hovered, mut background, mut border) in &mut tabs {
        let has_ready = lane_items(tab.0, production.faction_of(team))
            .into_iter()
            .any(|item| matches!(state_of(item), TileState::Ready));
        let selected = tab.0 == active.0;
        background.0 = if selected {
            PANEL_RAISED
        } else if hovered.get() {
            TILE_HOVERED
        } else {
            TILE
        };
        *border = BorderColor::all(if has_ready {
            GOOD.mix(&TEXT_MAIN, pulse * 0.5)
        } else if selected {
            ACCENT
        } else {
            FRAME
        });
    }
}

pub fn refresh_resources(
    local: Res<LocalTeam>,
    economies: Res<Economies>,
    mut credits_text: Query<&mut Text, (With<CreditsText>, Without<PowerText>)>,
    mut power_text: Query<(&mut Text, &mut TextColor), (With<PowerText>, Without<CreditsText>)>,
    mut power_fill: Query<
        (&mut Node, &mut BackgroundColor),
        (With<PowerBarFill>, Without<PowerBarDemand>),
    >,
    mut power_demand: Query<&mut Node, (With<PowerBarDemand>, Without<PowerBarFill>)>,
) {
    let economy = economies.of(local.0);
    if let Ok(mut text) = credits_text.single_mut() {
        text.set_if_neq(Text(format!("$ {}", economy.credits)));
    }
    let status_color = if economy.has_low_power() {
        DANGER
    } else if economy.power_supply - economy.power_demand < 25 {
        WARNING
    } else {
        GOOD
    };
    if let Ok((mut text, mut color)) = power_text.single_mut() {
        text.set_if_neq(Text(tr_fmt(
            "POWER {} used of {}",
            &[&economy.power_demand, &economy.power_supply],
        )));
        color.set_if_neq(TextColor(if economy.has_low_power() {
            DANGER
        } else {
            TEXT_MAIN
        }));
    }
    let scale = economy
        .power_supply
        .max(economy.power_demand)
        .max(POWER_BAR_SCALE) as f32;
    if let Ok((mut node, mut color)) = power_fill.single_mut() {
        let supplied = Val::Percent(economy.power_supply as f32 / scale * 100.0);
        set_field(&mut node, |node| &mut node.width, supplied);
        color.set_if_neq(BackgroundColor(status_color));
    }
    if let Ok(mut node) = power_demand.single_mut() {
        let demanded = Val::Percent(economy.power_demand as f32 / scale * 100.0);
        set_field(&mut node, |node| &mut node.left, demanded);
    }
}

fn harvest_activity(state: HarvestState) -> &'static str {
    match state {
        HarvestState::Holding => tr("idle"),
        HarvestState::SeekingField | HarvestState::TravellingToField(_) => {
            tr("heading to crystals")
        }
        HarvestState::Mining { .. } => tr("mining"),
        HarvestState::ReturningToRefinery(_) => tr("returning"),
        HarvestState::Unloading { .. } => tr("unloading"),
    }
}

fn describe_item(
    item: BuildItem,
    owned: &[BuildingKind],
    production: &Production,
    team: Team,
) -> (String, String) {
    let (role, build_secs, extra) = match item {
        BuildItem::Structure(kind) => {
            let spec = kind.spec();
            let power = if spec.power >= 0 {
                tr_fmt("+{} power", &[&spec.power])
            } else {
                tr_fmt("{} power", &[&spec.power])
            };
            (tr(spec.role), spec.build_secs, power)
        }
        BuildItem::Unit(kind) => {
            let spec = kind.spec();
            (
                spec.role,
                spec.build_secs,
                format!("{:.0} HP", spec.max_health),
            )
        }
        BuildItem::Research(upgrade) => {
            let spec = upgrade.spec();
            let status = if production.has_researched(team, upgrade) {
                tr("researched").to_string()
            } else {
                String::new()
            };
            (spec.role, spec.research_secs, status)
        }
    };
    let requirement = if !production.allows(team, item) {
        "\nNot available in this mission".to_string()
    } else {
        item.missing_requirement(owned)
            .map(|missing| format!("\n{}", tr_fmt("Requires {}", &[&tr(missing.spec().name)])))
            .unwrap_or_default()
    };
    (
        format!("{}  ${}", item.name(), item.cost()),
        tr_fmt(
            "{}\n{}s build, {}{}",
            &[&role, &format!("{build_secs:.0}"), &extra, &requirement],
        ),
    )
}

pub fn refresh_info_panel(
    mut commands: Commands,
    fonts: Res<UiFonts>,
    mut shown_roster: Local<Vec<(UnitKind, usize)>>,
    rosters: Query<Entity, With<InfoRoster>>,
    local: Res<LocalTeam>,
    asset_server: Res<AssetServer>,
    production: Res<Production>,
    buildings: Query<(&Building, &Team)>,
    hovered_tiles: Query<(&Hovered, &BuildTile)>,
    selected: Query<
        (
            Option<&Unit>,
            Option<&Building>,
            &Health,
            Option<&Harvester>,
        ),
        With<Selected>,
    >,
    mut icon: Query<(&mut ImageNode, &mut Node), (With<InfoIcon>, Without<InfoHealthFill>)>,
    mut title: Query<&mut Text, (With<InfoTitle>, Without<InfoBody>)>,
    mut body: Query<&mut Text, (With<InfoBody>, Without<InfoTitle>)>,
    mut health_fill: Query<(&mut Node, &mut BackgroundColor), With<InfoHealthFill>>,
) {
    let (
        Ok((mut icon_image, mut icon_node)),
        Ok(mut title),
        Ok(mut body),
        Ok((mut health_node, mut health_color)),
    ) = (
        icon.single_mut(),
        title.single_mut(),
        body.single_mut(),
        health_fill.single_mut(),
    )
    else {
        return;
    };
    let owned = owned_building_kinds(&buildings, local.0);
    let hovered = hovered_tiles
        .iter()
        .find(|(hovered, _)| hovered.get())
        .map(|(_, tile)| tile.0);
    let selection: Vec<_> = selected.iter().collect();
    let mut roster: Vec<(UnitKind, usize)> = Vec::new();

    let (shown_icon, heading, details, health) = if let Some(item) = hovered {
        let (heading, details) = describe_item(item, &owned, &production, local.0);
        (Some(icon_path(item)), heading, details, None)
    } else {
        match selection.as_slice() {
            [] => (
                None,
                tr("No selection").into(),
                tr("Select units with a click or a drag box.").into(),
                None,
            ),
            [(unit, building, health, harvester)] => {
                let name = unit
                    .map(|u| tr(u.0.spec().name))
                    .or(building.map(|b| tr(b.kind.spec().name)))
                    .unwrap_or_default();
                let path = unit
                    .map(|u| format!("icons/{}.png", u.0.spec().model_id))
                    .or(building.map(|b| format!("icons/{}.png", b.kind.spec().model_id)));
                let hp = format!("{:.0} / {:.0} HP", health.current.max(0.0), health.max);
                let details = match harvester {
                    Some(harvester) => tr_fmt(
                        "{}\nCargo {} / {}  ({})",
                        &[
                            &hp,
                            &harvester.cargo,
                            &HARVESTER_CAPACITY,
                            &harvest_activity(harvester.state),
                        ],
                    ),
                    None => hp,
                };
                (path, name.to_string(), details, Some(health.fraction()))
            }
            many => {
                for kind in many.iter().filter_map(|(unit, ..)| unit.map(|unit| unit.0)) {
                    match roster.iter_mut().find(|(listed, _)| *listed == kind) {
                        Some((_, count)) => *count += 1,
                        None => roster.push((kind, 1)),
                    }
                }
                roster.sort_by_key(|(kind, _)| UnitKind::ALL.iter().position(|k| k == kind));
                let average =
                    many.iter().map(|(_, _, h, _)| h.fraction()).sum::<f32>() / many.len() as f32;
                (
                    None,
                    tr_fmt("{} units", &[&many.len()]),
                    String::new(),
                    Some(average),
                )
            }
        }
    };

    title.set_if_neq(Text(heading));
    body.set_if_neq(Text(details));
    if *shown_roster != roster
        && let Ok(roster_node) = rosters.single()
    {
        commands.entity(roster_node).despawn_related::<Children>();
        commands.entity(roster_node).with_children(|row| {
            for &(kind, count) in &roster {
                spawn_roster_chip(row, &fonts, &asset_server, kind, count);
            }
        });
        *shown_roster = roster;
    }
    match shown_icon {
        Some(path) => {
            set_field(
                &mut icon_image,
                |icon| &mut icon.image,
                asset_server.load(path),
            );
            set_field(&mut icon_node, |node| &mut node.display, Display::Flex);
        }
        None => {
            set_field(&mut icon_node, |node| &mut node.display, Display::None);
        }
    }
    let fraction = health.unwrap_or(0.0);
    set_field(
        &mut health_node,
        |node| &mut node.width,
        Val::Percent(fraction * 100.0),
    );
    health_color.set_if_neq(BackgroundColor(match fraction {
        f if f > 0.6 => GOOD,
        f if f > 0.3 => WARNING,
        _ => DANGER,
    }));
}

pub fn refresh_notifications(
    notifications: Res<Notifications>,
    mut lines: Query<(&NotificationLine, &mut Text, &mut TextColor)>,
) {
    let visible: Vec<(&str, f32)> = notifications.visible().collect();
    for (line, mut text, mut color) in &mut lines {
        match visible.get(line.0) {
            Some((message, opacity)) => {
                if text.0 != *message {
                    text.0 = (*message).to_string();
                }
                color.set_if_neq(TextColor(ACCENT.with_alpha(*opacity)));
            }
            None => {
                if !text.0.is_empty() {
                    text.0.clear();
                }
            }
        }
    }
}

pub fn show_outcome(
    local: Res<LocalTeam>,
    view: LocalView,
    mut commands: Commands,
    fonts: Res<UiFonts>,
    outcome: Res<GameOutcome>,
    stats: Res<MatchStats>,
    economies: Res<Economies>,
    clock: Res<MatchClock>,
    mut overlay: Query<&mut Visibility, With<OutcomeOverlay>>,
    mut title: Query<(&mut Text, &mut TextColor), With<OutcomeTitle>>,
    tables: Query<Entity, With<OutcomeStats>>,
) {
    if !outcome.is_changed() {
        return;
    }
    let enemy_side = if local.0 == Team::Player {
        Team::Crimson
    } else {
        Team::Player
    };
    let banner = match *outcome {
        GameOutcome::Ongoing => None,
        _ if view.is_spectating() => Some((tr("THE BATTLE IS OVER"), local.0)),
        GameOutcome::Victory => Some((tr("VICTORY"), local.0)),
        GameOutcome::Defeat => Some((tr("DEFEAT"), enemy_side)),
    };
    if let Some((message, team)) = banner
        && let Ok((mut text, mut color)) = title.single_mut()
    {
        text.0 = message.into();
        color.0 = team.ui_color();
    }
    if banner.is_some()
        && let Ok(table) = tables.single()
    {
        let rows = outcome_rows(&stats, &economies, local.0);
        commands.entity(table).despawn_related::<Children>();
        commands.entity(table).with_children(|table| {
            spawn_outcome_table(table, &fonts, rows, clock.elapsed_secs, local.0);
        });
    }
    if let Ok(mut visibility) = overlay.single_mut() {
        *visibility = if banner.is_some() {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
}

pub fn refresh_objective(
    local: Res<LocalTeam>,
    level: Res<ActiveLevel>,
    clock: Res<MatchClock>,
    economies: Res<Economies>,
    mut lines: Query<&mut Text, With<ObjectiveText>>,
) {
    let status = objective_status(&level, &clock, &economies, local.0);
    for mut text in &mut lines {
        if text.0 != status {
            text.0 = status.clone();
        }
    }
}
