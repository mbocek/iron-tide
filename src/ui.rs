use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::text::{FontSize, FontSource};

use crate::catalog::{BuildingKind, HARVESTER_CAPACITY, Team, UnitKind};
use crate::components::*;
use crate::control_groups::spawn_group_bar;
use crate::economy::Economies;
use crate::levels::ActiveLevel;
use crate::match_flow::MatchClock;
use crate::menu::{MenuState, open_menu_from_button, spawn_menu};
use crate::minimap::{Minimap, jump_camera_on_drag, jump_camera_on_press};
use crate::notifications::Notifications;
use crate::outcome::{GameOutcome, objective_status};
use crate::perf_overlay::spawn_performance_overlay;
use crate::production::{BuildItem, Lane, Placement, Production, owned_building_kinds};
use crate::queue_bar::spawn_queue_bar;

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
const TILE_WIDTH: f32 = 134.0;
const TILE_HEIGHT: f32 = 106.0;
const TILE_ICON_SIZE: f32 = 72.0;
const MINIMAP_SIZE: f32 = 276.0;
const POWER_BAR_SCALE: i32 = 400;
const SHIFT_CLICK_BATCH: usize = 5;

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

#[derive(Component)]
pub struct NotificationLine(usize);

#[derive(Component)]
pub struct ControlsPanel;

#[derive(Component)]
pub struct ObjectiveText;

#[derive(Component)]
pub struct OutcomeOverlay;

#[derive(Component)]
pub struct OutcomeTitle;

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
    };
    format!("icons/{model_id}.png")
}

fn lane_items(lane: Lane) -> Vec<BuildItem> {
    match lane {
        Lane::Structures => BuildingKind::CONSTRUCTIBLE
            .map(BuildItem::Structure)
            .to_vec(),
        Lane::Infantry => UnitKind::INFANTRY.map(BuildItem::Unit).to_vec(),
        Lane::Vehicles => UnitKind::VEHICLES.map(BuildItem::Unit).to_vec(),
    }
}

fn lane_title(lane: Lane) -> &'static str {
    match lane {
        Lane::Structures => "BASE",
        Lane::Infantry => "INFANTRY",
        Lane::Vehicles => "VEHICLES",
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
            spawn_header(sidebar, fonts);
            spawn_minimap(sidebar, &minimap);
            spawn_resources(sidebar, fonts);
            spawn_tabs(sidebar, fonts);
            spawn_build_grid(sidebar, fonts, &asset_server);
            spawn_info_panel(sidebar, fonts);
        });

    spawn_notification_feed(&mut commands, fonts);
    spawn_queue_bar(&mut commands, fonts);
    spawn_group_bar(&mut commands, fonts);
    spawn_performance_overlay(&mut commands, fonts);
    spawn_menu(&mut commands, fonts);
    spawn_controls_help(&mut commands, fonts);
    spawn_outcome_overlay(&mut commands, fonts);
    commands.insert_resource(ActiveTab(Lane::Structures));
}

fn spawn_header(sidebar: &mut ChildSpawnerCommands, fonts: &UiFonts) {
    sidebar
        .spawn(Node {
            flex_direction: FlexDirection::Row,
            justify_content: JustifyContent::SpaceBetween,
            align_items: AlignItems::Center,
            ..default()
        })
        .with_children(|header| {
            header.spawn(fonts.heading("IRON TIDE", 30.0, ACCENT));
            header.spawn(fonts.heading("COALITION", 16.0, Team::Player.ui_color()));
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
            framed(Val::Px(MINIMAP_SIZE + 2.0), Val::Px(MINIMAP_SIZE + 2.0)),
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
                    align_items: AlignItems::Baseline,
                    ..default()
                })
                .with_children(|row| {
                    row.spawn((CreditsText, fonts.heading("$ 0", 26.0, ACCENT)));
                    row.spawn((PowerText, fonts.text("POWER 0 / 0", 16.0, TEXT_MAIN)));
                });
            panel
                .spawn((
                    Node {
                        width: Val::Percent(100.0),
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
) {
    sidebar
        .spawn(Node {
            flex_wrap: FlexWrap::Wrap,
            column_gap: Val::Px(8.0),
            row_gap: Val::Px(8.0),
            flex_grow: 1.0,
            align_content: AlignContent::FlexStart,
            ..default()
        })
        .with_children(|grid| {
            for item in Lane::ALL.into_iter().flat_map(lane_items) {
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
            padding: UiRect::all(Val::Px(4.0)),
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
        tile.spawn(Node {
            width: Val::Percent(100.0),
            justify_content: JustifyContent::SpaceBetween,
            padding: UiRect::horizontal(Val::Px(2.0)),
            ..default()
        })
        .with_children(|caption| {
            caption.spawn(fonts.heading(item.name(), 14.0, TEXT_MAIN));
            caption.spawn(fonts.heading(format!("${}", item.cost()), 14.0, ACCENT));
        });
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
            fonts.heading("", 15.0, GOOD),
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(3.0),
                right: Val::Px(6.0),
                ..default()
            },
        ));
    })
    .observe(click_build_tile);
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
                    ..default()
                },
                Visibility::Hidden,
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
            feed.spawn((
                ObjectiveText,
                fonts.heading("", 17.0, TEXT_MAIN),
                TextShadow::default(),
                Pickable::IGNORE,
            ));
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
        fonts.text("F1  controls", 15.0, TEXT_DIM),
        TextShadow::default(),
        Pickable::IGNORE,
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(12.0),
            bottom: Val::Px(8.0),
            ..default()
        },
    ));
    let controls = [
        ("Select", "left click / drag a box (Shift adds)"),
        ("Orders", "right click: move, attack, harvest"),
        ("Scroll", "WASD, arrows, screen edge, middle-drag"),
        ("Fast scroll", "hold Shift"),
        ("Minimap", "click or drag to jump"),
        ("Zoom / rotate", "mouse wheel / Q, E (R resets)"),
        ("Home", "Space or Home"),
        ("Stop / deselect", "X / Esc"),
        ("Groups", "Ctrl+1-9 save, 1-9 select, twice = jump"),
        ("Add a group", "Shift+1-9"),
        ("Graphics quality", "F2: Low / Medium / High"),
        ("Performance", "F3 overlay: FPS, CPU, GPU"),
        ("Save / load", "F5 quick save, F9 quick load"),
        ("Menu & settings", "F10 or Esc"),
        (
            "Build menu",
            "left click builds (Shift = 5x), right click cancels",
        ),
        (
            "Queue bar",
            "left click places a ready structure, right click cancels",
        ),
    ];
    commands
        .spawn((
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
        ))
        .with_children(|panel| {
            panel.spawn(fonts.heading("CONTROLS", 18.0, ACCENT));
            for (action, keys) in controls {
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
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.45)),
            Visibility::Hidden,
        ))
        .with_children(|overlay| {
            overlay.spawn((
                OutcomeTitle,
                fonts.heading("", 96.0, ACCENT),
                TextShadow::default(),
            ));
            overlay.spawn(fonts.text(
                "Open the menu (F10) to restart or return to the main menu.",
                24.0,
                TEXT_MAIN,
            ));
        });
}

pub fn toggle_controls_help(
    keys: Res<ButtonInput<KeyCode>>,
    mut panels: Query<&mut Visibility, With<ControlsPanel>>,
) {
    if !keys.just_pressed(KeyCode::F1) {
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
    keys: Res<ButtonInput<KeyCode>>,
    tiles: Query<&BuildTile>,
    mut production: ResMut<Production>,
    mut economies: ResMut<Economies>,
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
    if click.button == PointerButton::Secondary {
        production.cancel(&mut economies, Team::Player, item);
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
    let owned = owned_building_kinds(&buildings, Team::Player);
    if let BuildItem::Structure(kind) = item
        && production.lane(Team::Player, item.lane()).ready == Some(kind)
    {
        placement.active = Some(kind);
        return;
    }
    if let Some(missing) = item.missing_requirement(&owned) {
        notifications.push(format!("Requires {}", missing.spec().name));
        return;
    }
    let batch = if keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]) {
        SHIFT_CLICK_BATCH
    } else {
        1
    };
    for _ in 0..batch {
        if let Err(refusal) = production.enqueue(&mut economies, Team::Player, item, &owned) {
            notifications.push(refusal.message());
            return;
        }
    }
}

enum TileState {
    NotInMission,
    Locked,
    Ready,
    Building { fraction: f32, queued: usize },
    Queued(usize),
    Unaffordable,
    Available,
}

fn tile_state(
    item: BuildItem,
    production: &Production,
    owned: &[BuildingKind],
    credits: i32,
) -> TileState {
    if !production.allows(Team::Player, item) {
        return TileState::NotInMission;
    }
    if item.missing_requirement(owned).is_some() {
        return TileState::Locked;
    }
    let lane = production.lane(Team::Player, item.lane());
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
    let owned = owned_building_kinds(&buildings, Team::Player);
    let credits = economies.of(Team::Player).credits;
    let state_of = |item| tile_state(item, &production, &owned, credits);
    let pulse = (time.elapsed_secs() * 5.0).sin() * 0.5 + 0.5;

    for (tile, hovered, mut node, mut background, mut border) in &mut tiles {
        node.display = if tile.0.lane() == active.0 {
            Display::Flex
        } else {
            Display::None
        };
        let hovered = hovered.get();
        let ready = matches!(state_of(tile.0), TileState::Ready);
        background.0 = match (ready, hovered) {
            (true, _) => TILE_READY,
            (false, true) => TILE_HOVERED,
            (false, false) => TILE,
        };
        *border = BorderColor::all(if ready {
            GOOD.mix(&TEXT_MAIN, pulse * 0.5)
        } else if hovered {
            ACCENT
        } else {
            FRAME
        });
    }
    for (shade, mut node) in &mut shades {
        let covered = match state_of(shade.0) {
            TileState::NotInMission | TileState::Locked | TileState::Unaffordable => 1.0,
            TileState::Building { fraction, .. } => 1.0 - fraction,
            _ => 0.0,
        };
        node.height = Val::Percent(covered * 100.0);
    }
    for (badge, mut text, mut color) in &mut badges {
        let (label, tint) = match state_of(badge.0) {
            TileState::Ready => ("READY".to_string(), GOOD),
            TileState::Building { fraction, queued } if queued > 1 => (
                format!("{}%  x{queued}", (fraction * 100.0) as u32),
                TEXT_MAIN,
            ),
            TileState::Building { fraction, .. } => {
                (format!("{}%", (fraction * 100.0) as u32), TEXT_MAIN)
            }
            TileState::Queued(count) => (format!("x{count}"), TEXT_MAIN),
            TileState::Locked => ("LOCKED".to_string(), TEXT_DIM),
            TileState::NotInMission => ("N/A".to_string(), TEXT_DIM),
            TileState::Unaffordable | TileState::Available => (String::new(), TEXT_MAIN),
        };
        text.0 = label;
        color.0 = tint;
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
        let has_ready = lane_items(tab.0)
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
    economies: Res<Economies>,
    mut credits_text: Query<&mut Text, (With<CreditsText>, Without<PowerText>)>,
    mut power_text: Query<(&mut Text, &mut TextColor), (With<PowerText>, Without<CreditsText>)>,
    mut power_fill: Query<
        (&mut Node, &mut BackgroundColor),
        (With<PowerBarFill>, Without<PowerBarDemand>),
    >,
    mut power_demand: Query<&mut Node, (With<PowerBarDemand>, Without<PowerBarFill>)>,
) {
    let economy = economies.of(Team::Player);
    if let Ok(mut text) = credits_text.single_mut() {
        text.0 = format!("$ {}", economy.credits);
    }
    let status_color = if economy.has_low_power() {
        DANGER
    } else if economy.power_supply - economy.power_demand < 25 {
        WARNING
    } else {
        GOOD
    };
    if let Ok((mut text, mut color)) = power_text.single_mut() {
        text.0 = format!("POWER {} / {}", economy.power_demand, economy.power_supply);
        color.0 = if economy.has_low_power() {
            DANGER
        } else {
            TEXT_MAIN
        };
    }
    let scale = economy
        .power_supply
        .max(economy.power_demand)
        .max(POWER_BAR_SCALE) as f32;
    if let Ok((mut node, mut color)) = power_fill.single_mut() {
        node.width = Val::Percent(economy.power_supply as f32 / scale * 100.0);
        color.0 = status_color;
    }
    if let Ok(mut node) = power_demand.single_mut() {
        node.left = Val::Percent(economy.power_demand as f32 / scale * 100.0);
    }
}

fn harvest_activity(state: HarvestState) -> &'static str {
    match state {
        HarvestState::Holding => "idle",
        HarvestState::SeekingField | HarvestState::TravellingToField(_) => "heading to crystals",
        HarvestState::Mining { .. } => "mining",
        HarvestState::ReturningToRefinery(_) => "returning",
        HarvestState::Unloading { .. } => "unloading",
    }
}

fn describe_item(
    item: BuildItem,
    owned: &[BuildingKind],
    production: &Production,
) -> (String, String) {
    let (role, build_secs, extra) = match item {
        BuildItem::Structure(kind) => {
            let spec = kind.spec();
            let power = if spec.power >= 0 {
                format!("+{} power", spec.power)
            } else {
                format!("{} power", spec.power)
            };
            (spec.role, spec.build_secs, power)
        }
        BuildItem::Unit(kind) => {
            let spec = kind.spec();
            (
                spec.role,
                spec.build_secs,
                format!("{:.0} HP", spec.max_health),
            )
        }
    };
    let requirement = if !production.allows(Team::Player, item) {
        "\nNot available in this mission".to_string()
    } else {
        item.missing_requirement(owned)
            .map(|missing| format!("\nRequires {}", missing.spec().name))
            .unwrap_or_default()
    };
    (
        format!("{}  ${}", item.name(), item.cost()),
        format!("{role}\n{build_secs:.0}s build, {extra}{requirement}"),
    )
}

pub fn refresh_info_panel(
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
    mut icon: Query<(&mut ImageNode, &mut Visibility), With<InfoIcon>>,
    mut title: Query<&mut Text, (With<InfoTitle>, Without<InfoBody>)>,
    mut body: Query<&mut Text, (With<InfoBody>, Without<InfoTitle>)>,
    mut health_fill: Query<(&mut Node, &mut BackgroundColor), With<InfoHealthFill>>,
) {
    let (
        Ok((mut icon_image, mut icon_visibility)),
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
    let owned = owned_building_kinds(&buildings, Team::Player);
    let hovered = hovered_tiles
        .iter()
        .find(|(hovered, _)| hovered.get())
        .map(|(_, tile)| tile.0);
    let selection: Vec<_> = selected.iter().collect();

    let (shown_icon, heading, details, health) = if let Some(item) = hovered {
        let (heading, details) = describe_item(item, &owned, &production);
        (Some(icon_path(item)), heading, details, None)
    } else {
        match selection.as_slice() {
            [] => (
                None,
                "No selection".into(),
                "Select units with a click or a drag box.".into(),
                None,
            ),
            [(unit, building, health, harvester)] => {
                let name = unit
                    .map(|u| u.0.spec().name)
                    .or(building.map(|b| b.kind.spec().name))
                    .unwrap_or_default();
                let path = unit
                    .map(|u| format!("icons/{}.png", u.0.spec().model_id))
                    .or(building.map(|b| format!("icons/{}.png", b.kind.spec().model_id)));
                let hp = format!("{:.0} / {:.0} HP", health.current.max(0.0), health.max);
                let details = match harvester {
                    Some(harvester) => format!(
                        "{hp}\nCargo {} / {}  ({})",
                        harvester.cargo,
                        HARVESTER_CAPACITY,
                        harvest_activity(harvester.state)
                    ),
                    None => hp,
                };
                (path, name.to_string(), details, Some(health.fraction()))
            }
            many => {
                let mut counts: Vec<(&str, usize)> = Vec::new();
                for (unit, ..) in many {
                    let name = unit.map_or("Structure", |u| u.0.spec().name);
                    match counts.iter_mut().find(|(n, _)| *n == name) {
                        Some((_, count)) => *count += 1,
                        None => counts.push((name, 1)),
                    }
                }
                let summary = counts
                    .iter()
                    .map(|(name, count)| format!("{count}x {name}"))
                    .collect::<Vec<_>>()
                    .join("\n");
                let average =
                    many.iter().map(|(_, _, h, _)| h.fraction()).sum::<f32>() / many.len() as f32;
                (
                    None,
                    format!("{} units", many.len()),
                    summary,
                    Some(average),
                )
            }
        }
    };

    title.0 = heading;
    body.0 = details;
    match shown_icon {
        Some(path) => {
            icon_image.image = asset_server.load(path);
            *icon_visibility = Visibility::Inherited;
        }
        None => *icon_visibility = Visibility::Hidden,
    }
    let fraction = health.unwrap_or(0.0);
    health_node.width = Val::Percent(fraction * 100.0);
    health_color.0 = match fraction {
        f if f > 0.6 => GOOD,
        f if f > 0.3 => WARNING,
        _ => DANGER,
    };
}

pub fn refresh_notifications(
    notifications: Res<Notifications>,
    mut lines: Query<(&NotificationLine, &mut Text, &mut TextColor)>,
) {
    let visible: Vec<(&str, f32)> = notifications.visible().collect();
    for (line, mut text, mut color) in &mut lines {
        match visible.get(line.0) {
            Some((message, opacity)) => {
                text.0 = (*message).to_string();
                color.0 = ACCENT.with_alpha(*opacity);
            }
            None => text.0.clear(),
        }
    }
}

pub fn show_outcome(
    outcome: Res<GameOutcome>,
    mut overlay: Query<&mut Visibility, With<OutcomeOverlay>>,
    mut title: Query<(&mut Text, &mut TextColor), With<OutcomeTitle>>,
) {
    if !outcome.is_changed() {
        return;
    }
    let banner = match *outcome {
        GameOutcome::Ongoing => None,
        GameOutcome::Victory => Some(("VICTORY", Team::Player)),
        GameOutcome::Defeat => Some(("DEFEAT", Team::Crimson)),
    };
    if let Some((message, team)) = banner
        && let Ok((mut text, mut color)) = title.single_mut()
    {
        text.0 = message.into();
        color.0 = team.ui_color();
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
    level: Res<ActiveLevel>,
    clock: Res<MatchClock>,
    economies: Res<Economies>,
    mut lines: Query<&mut Text, With<ObjectiveText>>,
) {
    let status = format!(
        "OBJECTIVE: {}",
        objective_status(&level, &clock, &economies)
    );
    for mut text in &mut lines {
        if text.0 != status {
            text.0 = status.clone();
        }
    }
}
