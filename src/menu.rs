use bevy::picking::hover::Hovered;
use bevy::prelude::*;

use crate::components::Selected;
use crate::match_flow::GameState;
use crate::production::Placement;
use crate::savegame::{
    NAMED_SLOTS, QUICK_SAVE_SLOT, SaveDirectory, SaveRequests, slot_label, slot_summary,
};
use crate::settings::{
    GameSettings, GameSpeed, HealthBars, QualityPreset, ScrollSpeed, ShadowQuality,
    VegetationDetail,
};
use crate::ui::{ACCENT, FRAME, FRAME_BRIGHT, PANEL, TEXT_DIM, TEXT_MAIN, UiFonts};

const MENU_WIDTH: f32 = 620.0;
const VALUE_BUTTON_WIDTH: f32 = 190.0;
const BUTTON_IDLE: Color = Color::srgb(0.13, 0.15, 0.17);
const BUTTON_HOVERED: Color = Color::srgb(0.19, 0.22, 0.25);

#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
enum MenuPage {
    #[default]
    Game,
    Graphics,
    Gameplay,
    SaveLoad,
}

impl MenuPage {
    const ALL: [MenuPage; 4] = [
        MenuPage::Game,
        MenuPage::Graphics,
        MenuPage::Gameplay,
        MenuPage::SaveLoad,
    ];

    fn title(self) -> &'static str {
        match self {
            MenuPage::Game => "GAME",
            MenuPage::Graphics => "GRAPHICS",
            MenuPage::Gameplay => "GAMEPLAY",
            MenuPage::SaveLoad => "SAVE / LOAD",
        }
    }
}

#[derive(Resource, Default)]
pub struct MenuState {
    open: bool,
    page: MenuPage,
    needs_rebuild: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Setting {
    Preset,
    Shadows,
    AmbientOcclusion,
    Bloom,
    AntiAliasing,
    Vegetation,
    Wind,
    Vsync,
    Fullscreen,
    GameSpeed,
    EdgeScroll,
    ScrollSpeed,
    DamageNumbers,
    HealthBars,
    PerformanceOverlay,
}

const GRAPHICS_SETTINGS: [Setting; 9] = [
    Setting::Preset,
    Setting::Shadows,
    Setting::AmbientOcclusion,
    Setting::Bloom,
    Setting::AntiAliasing,
    Setting::Vegetation,
    Setting::Wind,
    Setting::Vsync,
    Setting::Fullscreen,
];

const GAMEPLAY_SETTINGS: [Setting; 6] = [
    Setting::GameSpeed,
    Setting::EdgeScroll,
    Setting::ScrollSpeed,
    Setting::DamageNumbers,
    Setting::HealthBars,
    Setting::PerformanceOverlay,
];

fn on_off(value: bool) -> String {
    if value { "On".into() } else { "Off".into() }
}

/// Steps through a fixed list of choices, wrapping at both ends.
fn step<T: Copy + PartialEq>(choices: &[T], current: T, forward: bool) -> T {
    let index = choices
        .iter()
        .position(|&choice| choice == current)
        .unwrap_or(0);
    let next = if forward {
        (index + 1) % choices.len()
    } else {
        (index + choices.len() - 1) % choices.len()
    };
    choices[next]
}

impl Setting {
    fn label(self) -> &'static str {
        match self {
            Setting::Preset => "Quality preset",
            Setting::Shadows => "Shadows",
            Setting::AmbientOcclusion => "Ambient occlusion",
            Setting::Bloom => "Glow (bloom)",
            Setting::AntiAliasing => "Anti-aliasing (SMAA)",
            Setting::Vegetation => "Grass and flowers",
            Setting::Wind => "Wind in vegetation",
            Setting::Vsync => "VSync",
            Setting::Fullscreen => "Fullscreen",
            Setting::GameSpeed => "Game speed",
            Setting::EdgeScroll => "Edge scrolling",
            Setting::ScrollSpeed => "Scroll speed",
            Setting::DamageNumbers => "Damage numbers",
            Setting::HealthBars => "Health bars",
            Setting::PerformanceOverlay => "Performance overlay (F3)",
        }
    }

    fn value(self, settings: &GameSettings) -> String {
        match self {
            Setting::Preset => format!("{:?}", settings.preset),
            Setting::Shadows => format!("{:?}", settings.shadows),
            Setting::AmbientOcclusion => on_off(settings.ambient_occlusion),
            Setting::Bloom => on_off(settings.bloom),
            Setting::AntiAliasing => on_off(settings.anti_aliasing),
            Setting::Vegetation => format!("{:?}", settings.vegetation),
            Setting::Wind => on_off(settings.wind),
            Setting::Vsync => on_off(settings.vsync),
            Setting::Fullscreen => on_off(settings.fullscreen),
            Setting::GameSpeed => format!("{:.2}x", settings.game_speed_factor()),
            Setting::EdgeScroll => on_off(settings.edge_scroll),
            Setting::ScrollSpeed => format!("{:?}", settings.scroll_speed),
            Setting::DamageNumbers => on_off(settings.damage_numbers),
            Setting::HealthBars => match settings.health_bars {
                HealthBars::Always => "Always".into(),
                HealthBars::DamagedOrSelected => "Damaged / selected".into(),
                HealthBars::SelectedOnly => "Selected only".into(),
            },
            Setting::PerformanceOverlay => on_off(settings.performance_overlay),
        }
    }

    fn is_detail_option(self) -> bool {
        matches!(
            self,
            Setting::Shadows
                | Setting::AmbientOcclusion
                | Setting::Bloom
                | Setting::Vegetation
                | Setting::Wind
        )
    }

    fn change(self, settings: &mut GameSettings, forward: bool) {
        match self {
            Setting::Preset => {
                let presets = [
                    QualityPreset::Low,
                    QualityPreset::Medium,
                    QualityPreset::High,
                ];
                let next = step(&presets, settings.preset, forward);
                settings.apply_preset(next);
                return;
            }
            Setting::Shadows => {
                let levels = [
                    ShadowQuality::Off,
                    ShadowQuality::Low,
                    ShadowQuality::Medium,
                    ShadowQuality::High,
                ];
                settings.shadows = step(&levels, settings.shadows, forward);
            }
            Setting::AmbientOcclusion => settings.ambient_occlusion = !settings.ambient_occlusion,
            Setting::Bloom => settings.bloom = !settings.bloom,
            Setting::AntiAliasing => settings.anti_aliasing = !settings.anti_aliasing,
            Setting::Vegetation => {
                let levels = [
                    VegetationDetail::Off,
                    VegetationDetail::Near,
                    VegetationDetail::Far,
                ];
                settings.vegetation = step(&levels, settings.vegetation, forward);
            }
            Setting::Wind => settings.wind = !settings.wind,
            Setting::Vsync => settings.vsync = !settings.vsync,
            Setting::Fullscreen => settings.fullscreen = !settings.fullscreen,
            Setting::GameSpeed => {
                let speeds = [
                    GameSpeed::Slow,
                    GameSpeed::Normal,
                    GameSpeed::Fast,
                    GameSpeed::Fastest,
                ];
                settings.game_speed = step(&speeds, settings.game_speed, forward);
            }
            Setting::EdgeScroll => settings.edge_scroll = !settings.edge_scroll,
            Setting::ScrollSpeed => {
                let speeds = [ScrollSpeed::Slow, ScrollSpeed::Normal, ScrollSpeed::Fast];
                settings.scroll_speed = step(&speeds, settings.scroll_speed, forward);
            }
            Setting::DamageNumbers => settings.damage_numbers = !settings.damage_numbers,
            Setting::HealthBars => {
                let modes = [
                    HealthBars::Always,
                    HealthBars::DamagedOrSelected,
                    HealthBars::SelectedOnly,
                ];
                settings.health_bars = step(&modes, settings.health_bars, forward);
            }
            Setting::PerformanceOverlay => {
                settings.performance_overlay = !settings.performance_overlay
            }
        }
        if self.is_detail_option() {
            settings.preset = QualityPreset::Custom;
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum MenuAction {
    Resume,
    Restart,
    MainMenu,
    Quit,
    Save(usize),
    Load(usize),
}

#[derive(Component)]
pub struct MenuRoot;

#[derive(Component)]
pub struct MenuContent;

#[derive(Component)]
pub struct MenuTab(MenuPage);

#[derive(Component)]
pub struct SettingButton(Setting);

#[derive(Component)]
pub struct SettingValue(Setting);

#[derive(Component)]
pub struct ActionButton(MenuAction);

pub fn spawn_menu(commands: &mut Commands, fonts: &UiFonts) {
    commands
        .spawn((
            MenuRoot,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                right: Val::Px(0.0),
                top: Val::Px(0.0),
                bottom: Val::Px(0.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.55)),
            GlobalZIndex(10),
            Visibility::Hidden,
        ))
        .with_children(|backdrop| {
            backdrop
                .spawn((
                    Node {
                        width: Val::Px(MENU_WIDTH),
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::all(Val::Px(18.0)),
                        row_gap: Val::Px(12.0),
                        border: UiRect::all(Val::Px(2.0)),
                        ..default()
                    },
                    BackgroundColor(PANEL),
                    BorderColor::all(FRAME_BRIGHT),
                ))
                .with_children(|panel| {
                    panel
                        .spawn(Node {
                            justify_content: JustifyContent::SpaceBetween,
                            align_items: AlignItems::Center,
                            ..default()
                        })
                        .with_children(|header| {
                            header.spawn(fonts.heading("IRON TIDE", 30.0, ACCENT));
                            header.spawn(fonts.text(
                                "paused  -  Esc / F10 to resume",
                                15.0,
                                TEXT_DIM,
                            ));
                        });
                    panel
                        .spawn(Node {
                            column_gap: Val::Px(6.0),
                            ..default()
                        })
                        .with_children(|tabs| {
                            for page in MenuPage::ALL {
                                tabs.spawn((
                                    MenuTab(page),
                                    Hovered::default(),
                                    Node {
                                        flex_grow: 1.0,
                                        height: Val::Px(32.0),
                                        justify_content: JustifyContent::Center,
                                        align_items: AlignItems::Center,
                                        border: UiRect::all(Val::Px(1.0)),
                                        ..default()
                                    },
                                    BackgroundColor(BUTTON_IDLE),
                                    BorderColor::all(FRAME),
                                ))
                                .with_children(|tab| {
                                    tab.spawn(fonts.heading(page.title(), 15.0, TEXT_MAIN));
                                })
                                .observe(select_page);
                            }
                        });
                    panel.spawn((
                        MenuContent,
                        Node {
                            flex_direction: FlexDirection::Column,
                            row_gap: Val::Px(6.0),
                            min_height: Val::Px(380.0),
                            ..default()
                        },
                    ));
                });
        });
}

pub fn toggle_menu(
    keys: Res<ButtonInput<KeyCode>>,
    placement: Res<Placement>,
    selected: Query<(), With<Selected>>,
    mut menu: ResMut<MenuState>,
) {
    let escape_is_free = placement.active.is_none() && selected.is_empty();
    let wants_toggle = keys.just_pressed(KeyCode::F10)
        || (keys.just_pressed(KeyCode::Escape) && (menu.open || escape_is_free));
    if wants_toggle {
        let open = !menu.open;
        set_open(&mut menu, open);
    }
}

fn set_open(menu: &mut MenuState, open: bool) {
    menu.open = open;
    menu.needs_rebuild = open;
}

/// A fresh match starts with the pause menu closed and time running.
pub fn close_menu(mut menu: ResMut<MenuState>) {
    *menu = MenuState::default();
}

pub fn open_menu_from_button(mut menu: ResMut<MenuState>) {
    set_open(&mut menu, true);
}

/// The simulation runs on virtual time, so pausing it freezes the battle while the menu is open.
pub fn pause_while_menu_open(
    menu: Res<MenuState>,
    mut time: ResMut<Time<Virtual>>,
    mut roots: Query<&mut Visibility, With<MenuRoot>>,
) {
    if !menu.is_changed() {
        return;
    }
    if menu.open {
        time.pause();
    } else {
        time.unpause();
    }
    for mut visibility in &mut roots {
        *visibility = if menu.open {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
}

fn select_page(click: On<PointerClick>, tabs: Query<&MenuTab>, mut menu: ResMut<MenuState>) {
    if let Ok(tab) = tabs.get(click.entity) {
        menu.page = tab.0;
        menu.needs_rebuild = true;
    }
}

fn change_setting(
    click: On<PointerClick>,
    buttons: Query<&SettingButton>,
    mut settings: ResMut<GameSettings>,
) {
    let Ok(button) = buttons.get(click.entity) else {
        return;
    };
    let forward = match click.button {
        PointerButton::Primary => true,
        PointerButton::Secondary => false,
        PointerButton::Middle => return,
    };
    button.0.change(&mut settings, forward);
}

fn run_action(
    click: On<PointerClick>,
    buttons: Query<&ActionButton>,
    mut menu: ResMut<MenuState>,
    mut requests: ResMut<SaveRequests>,
    mut next_state: ResMut<NextState<GameState>>,
    mut exit: MessageWriter<AppExit>,
) {
    let Ok(button) = buttons.get(click.entity) else {
        return;
    };
    if click.button != PointerButton::Primary {
        return;
    }
    match button.0 {
        MenuAction::Resume => set_open(&mut menu, false),
        MenuAction::Restart => {
            set_open(&mut menu, false);
            next_state.set(GameState::Restarting);
        }
        MenuAction::MainMenu => {
            set_open(&mut menu, false);
            next_state.set(GameState::MainMenu);
        }
        MenuAction::Quit => {
            exit.write(AppExit::Success);
        }
        MenuAction::Save(slot) => {
            requests.save = Some(slot);
            menu.needs_rebuild = true;
        }
        MenuAction::Load(slot) => {
            requests.load = Some(slot);
            set_open(&mut menu, false);
        }
    }
}

fn spawn_button(
    parent: &mut ChildSpawnerCommands,
    fonts: &UiFonts,
    label: impl Into<String>,
    width: f32,
) -> Entity {
    parent
        .spawn((
            Hovered::default(),
            Node {
                width: Val::Px(width),
                height: Val::Px(30.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(BUTTON_IDLE),
            BorderColor::all(FRAME),
        ))
        .with_children(|button| {
            button.spawn(fonts.heading(label, 15.0, TEXT_MAIN));
        })
        .id()
}

fn spawn_setting_row(
    content: &mut ChildSpawnerCommands,
    fonts: &UiFonts,
    settings: &GameSettings,
    setting: Setting,
) {
    content
        .spawn(Node {
            justify_content: JustifyContent::SpaceBetween,
            align_items: AlignItems::Center,
            ..default()
        })
        .with_children(|row| {
            row.spawn(fonts.text(setting.label(), 17.0, TEXT_MAIN));
            row.spawn((
                SettingButton(setting),
                Hovered::default(),
                Node {
                    width: Val::Px(VALUE_BUTTON_WIDTH),
                    height: Val::Px(30.0),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    border: UiRect::all(Val::Px(1.0)),
                    ..default()
                },
                BackgroundColor(BUTTON_IDLE),
                BorderColor::all(FRAME),
            ))
            .with_children(|button| {
                button.spawn((
                    SettingValue(setting),
                    fonts.heading(setting.value(settings), 15.0, ACCENT),
                ));
            })
            .observe(change_setting);
        });
}

fn spawn_slot_row(
    content: &mut ChildSpawnerCommands,
    fonts: &UiFonts,
    directory: &SaveDirectory,
    slot: usize,
) {
    let summary = slot_summary(directory, slot);
    content
        .spawn(Node {
            justify_content: JustifyContent::SpaceBetween,
            align_items: AlignItems::Center,
            column_gap: Val::Px(8.0),
            ..default()
        })
        .with_children(|row| {
            row.spawn(Node {
                flex_direction: FlexDirection::Column,
                flex_grow: 1.0,
                ..default()
            })
            .with_children(|info| {
                info.spawn(fonts.heading(slot_label(slot), 17.0, TEXT_MAIN));
                let (detail, color) = match &summary {
                    Some(summary) => (summary.clone(), TEXT_DIM),
                    None => ("empty".to_string(), FRAME_BRIGHT),
                };
                info.spawn(fonts.text(detail, 14.0, color));
            });
            let save = spawn_button(row, fonts, "Save", 90.0);
            row.commands()
                .entity(save)
                .insert(ActionButton(MenuAction::Save(slot)))
                .observe(run_action);
            if summary.is_some() {
                let load = spawn_button(row, fonts, "Load", 90.0);
                row.commands()
                    .entity(load)
                    .insert(ActionButton(MenuAction::Load(slot)))
                    .observe(run_action);
            }
        });
}

pub fn rebuild_menu_page(
    mut commands: Commands,
    mut menu: ResMut<MenuState>,
    fonts: Option<Res<UiFonts>>,
    settings: Res<GameSettings>,
    save_directory: Res<SaveDirectory>,
    contents: Query<Entity, With<MenuContent>>,
) {
    if !menu.needs_rebuild {
        return;
    }
    let (Some(fonts), Ok(content)) = (fonts, contents.single()) else {
        return;
    };
    menu.needs_rebuild = false;
    commands.entity(content).despawn_related::<Children>();
    let page = menu.page;
    commands.entity(content).with_children(|content| match page {
        MenuPage::Game => {
            content.spawn(fonts.text(
                "Battle is paused. Graphics and gameplay options are saved automatically.\nF5 quick save, F9 quick load, F3 performance overlay.",
                16.0,
                TEXT_DIM,
            ));
            let actions = [
                ("Resume", MenuAction::Resume),
                ("Restart mission", MenuAction::Restart),
                ("Back to main menu", MenuAction::MainMenu),
                ("Quit to desktop", MenuAction::Quit),
            ];
            for (label, action) in actions {
                let button = spawn_button(content, &fonts, label, 240.0);
                content.commands().entity(button).insert(ActionButton(action)).observe(run_action);
            }
        }
        MenuPage::Graphics => {
            for setting in GRAPHICS_SETTINGS {
                spawn_setting_row(content, &fonts, &settings, setting);
            }
            content.spawn(fonts.text("Left click: next value, right click: previous.", 14.0, TEXT_DIM));
        }
        MenuPage::Gameplay => {
            for setting in GAMEPLAY_SETTINGS {
                spawn_setting_row(content, &fonts, &settings, setting);
            }
        }
        MenuPage::SaveLoad => {
            spawn_slot_row(content, &fonts, &save_directory, QUICK_SAVE_SLOT);
            for slot in NAMED_SLOTS {
                spawn_slot_row(content, &fonts, &save_directory, slot);
            }
        }
    });
}

pub fn refresh_menu(
    menu: Res<MenuState>,
    settings: Res<GameSettings>,
    mut values: Query<(&SettingValue, &mut Text)>,
    mut tabs: Query<(&MenuTab, &mut BorderColor, &mut BackgroundColor, &Hovered)>,
    mut buttons: Query<
        (&Hovered, &mut BackgroundColor),
        (
            Without<MenuTab>,
            Or<(With<SettingButton>, With<ActionButton>)>,
        ),
    >,
) {
    if !menu.open {
        return;
    }
    for (value, mut text) in &mut values {
        let current = value.0.value(&settings);
        if text.0 != current {
            text.0 = current;
        }
    }
    for (tab, mut border, mut background, hovered) in &mut tabs {
        let active = tab.0 == menu.page;
        *border = BorderColor::all(if active { ACCENT } else { FRAME });
        background.0 = if hovered.get() || active {
            BUTTON_HOVERED
        } else {
            BUTTON_IDLE
        };
    }
    for (hovered, mut background) in &mut buttons {
        background.0 = if hovered.get() {
            BUTTON_HOVERED
        } else {
            BUTTON_IDLE
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stepping_wraps_in_both_directions() {
        let choices = [1, 2, 3];
        assert_eq!(step(&choices, 3, true), 1);
        assert_eq!(step(&choices, 1, false), 3);
    }

    #[test]
    fn touching_a_detail_option_switches_to_a_custom_preset() {
        let mut settings = GameSettings::default();
        Setting::Bloom.change(&mut settings, true);
        assert_eq!(settings.preset, QualityPreset::Custom);
        Setting::Preset.change(&mut settings, true);
        assert_ne!(settings.preset, QualityPreset::Custom);
    }

    #[test]
    fn gameplay_options_do_not_touch_the_graphics_preset() {
        let mut settings = GameSettings::default();
        Setting::GameSpeed.change(&mut settings, true);
        assert_eq!(settings.preset, QualityPreset::High);
        assert_eq!(settings.game_speed, GameSpeed::Fast);
    }
}
