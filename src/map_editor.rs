use std::path::PathBuf;

use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::prelude::*;

use crate::camera::Cursor;
use crate::catalog::{BuildingKind, Team};
use crate::i18n::{tr, tr_fmt, tr_owned};
use crate::level_editing::{
    LevelText, add_enemy, cycle_difficulty, cycle_objective, erase_from, move_side, remove_enemy,
    type_into,
};
use crate::levels::{ActiveLevel, LevelDefinition, custom_levels_directory, save_custom_level};
use crate::map::{
    GameMap, PlayfieldGround, cell_center, cells_by_distance, footprint_center, opposite_cell,
    playfield_ground_mesh, world_to_cell,
};
use crate::map_edits::{Brush, Ground, MapEdit, MapPainter};
use crate::match_flow::GameState;
use crate::ui::{ACCENT, FRAME, GOOD, PANEL, SIDEBAR_WIDTH, TEXT_DIM, TEXT_MAIN, UiFonts};

const BRUSH_SIZES: [i32; 3] = [0, 1, 2];
const BRUSH_KEYS: [KeyCode; 4] = [
    KeyCode::Digit1,
    KeyCode::Digit2,
    KeyCode::Digit3,
    KeyCode::Digit4,
];
const BUTTON_IDLE: Color = Color::srgb(0.11, 0.13, 0.15);
const BUTTON_ACTIVE: Color = Color::srgb(0.17, 0.24, 0.32);

/// The open map editor: the level being repainted, where it is saved and how the brush is set.
#[derive(Resource)]
pub struct MapEditor {
    pub brush: Brush,
    /// Cells painted around the one under the cursor; zero paints a single cell.
    pub radius: i32,
    /// Paint the opposite cell too, so both halves of the map stay equally fair.
    pub mirror: bool,
    /// Where the level is saved; none until the first save creates a new custom level file.
    pub file: Option<PathBuf>,
    pub message: Option<String>,
    pub page: EditorPage,
    /// The text the keyboard writes into; brush keys and camera keys rest meanwhile.
    pub typing: Option<LevelText>,
    panel_outdated: bool,
}

impl MapEditor {
    pub fn new(file: Option<PathBuf>) -> MapEditor {
        MapEditor {
            brush: Brush::Crystal,
            radius: 1,
            mirror: true,
            file,
            message: None,
            page: EditorPage::Paint,
            typing: None,
            panel_outdated: true,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EditorPage {
    Paint,
    Level,
}

/// Opens the map editor on a level; `file` is the custom level it overwrites when saved.
pub fn open_map_editor(
    commands: &mut Commands,
    next_state: &mut NextState<GameState>,
    level: LevelDefinition,
    file: Option<PathBuf>,
) {
    commands.insert_resource(ActiveLevel(level));
    commands.insert_resource(MapEditor::new(file));
    next_state.set(GameState::MapEditor);
}

#[derive(Component, Clone, Copy)]
pub enum EditorButton {
    Page(EditorPage),
    Brush(Brush),
    Radius(i32),
    Mirror,
    Type(LevelText),
    Objective,
    MoveSide(usize),
    Difficulty(usize),
    AddEnemy,
    RemoveEnemy,
    Faction,
    PlayTest,
    Save,
    Exit,
}

#[derive(Component)]
pub struct MapEditorPanel;

/// Builds the panel again after anything on it changed; it is a few dozen nodes.
pub fn rebuild_map_editor_panel(
    mut commands: Commands,
    fonts: Res<UiFonts>,
    level: Res<ActiveLevel>,
    mut editor: ResMut<MapEditor>,
    panels: Query<Entity, With<MapEditorPanel>>,
) {
    if !editor.panel_outdated {
        return;
    }
    editor.panel_outdated = false;
    for panel in &panels {
        commands.entity(panel).despawn();
    }
    commands
        .spawn((
            MapEditorPanel,
            DespawnOnExit(GameState::MapEditor),
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(0.0),
                right: Val::Px(0.0),
                bottom: Val::Px(0.0),
                width: Val::Px(SIDEBAR_WIDTH),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(14.0)),
                row_gap: Val::Px(7.0),
                border: UiRect::left(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(PANEL),
            BorderColor::all(FRAME),
        ))
        .with_children(|panel| {
            panel.spawn(fonts.heading(tr("MAP EDITOR"), 24.0, ACCENT));
            row(panel, |row| {
                for (page, label) in [
                    (EditorPage::Paint, tr("PAINT")),
                    (EditorPage::Level, tr("LEVEL")),
                ] {
                    spawn_button(
                        row,
                        &fonts,
                        label,
                        EditorButton::Page(page),
                        editor.page == page,
                    );
                }
            });
            match editor.page {
                EditorPage::Paint => spawn_paint_page(panel, &fonts, &editor),
                EditorPage::Level => spawn_level_page(panel, &fonts, &level.0, editor.typing),
            }
            spawn_button(panel, &fonts, tr("PLAY IT"), EditorButton::PlayTest, false);
            spawn_button(panel, &fonts, tr("SAVE"), EditorButton::Save, false);
            spawn_button(panel, &fonts, tr("BACK TO MENU"), EditorButton::Exit, false);
            if let Some(message) = &editor.message {
                panel.spawn(fonts.text(message.clone(), 14.0, GOOD));
            }
        });
}

fn spawn_paint_page(panel: &mut ChildSpawnerCommands, fonts: &UiFonts, editor: &MapEditor) {
    panel.spawn(fonts.heading(tr("BRUSH"), 15.0, TEXT_DIM));
    for (index, brush) in Brush::ALL.into_iter().enumerate() {
        let label = format!("{}  {}", index + 1, tr(brush.name()));
        spawn_button(
            panel,
            fonts,
            &label,
            EditorButton::Brush(brush),
            editor.brush == brush,
        );
    }
    panel.spawn(fonts.heading(tr("SIZE"), 15.0, TEXT_DIM));
    row(panel, |row| {
        for radius in BRUSH_SIZES {
            let label = (2 * radius + 1).to_string();
            spawn_button(
                row,
                fonts,
                &label,
                EditorButton::Radius(radius),
                editor.radius == radius,
            );
        }
    });
    let mirror = if editor.mirror {
        tr("Mirror: on")
    } else {
        tr("Mirror: off")
    };
    spawn_button(panel, fonts, mirror, EditorButton::Mirror, editor.mirror);
    panel.spawn(fonts.text(
        tr("Left button paints, right button clears. Bases stay open. Mirroring keeps both halves of the map equally fair."),
        14.0,
        TEXT_DIM,
    ));
}

fn spawn_level_page(
    panel: &mut ChildSpawnerCommands,
    fonts: &UiFonts,
    level: &LevelDefinition,
    typing: Option<LevelText>,
) {
    for (target, label, text) in [
        (LevelText::Name, tr("Name"), &level.name),
        (LevelText::Briefing, tr("Briefing"), &level.briefing),
    ] {
        panel.spawn(fonts.heading(label, 15.0, TEXT_DIM));
        let cursor = if typing == Some(target) { "_" } else { "" };
        spawn_button(
            panel,
            fonts,
            &format!("{text}{cursor}"),
            EditorButton::Type(target),
            typing == Some(target),
        );
    }
    panel.spawn(fonts.heading(tr("Objective"), 15.0, TEXT_DIM));
    spawn_button(
        panel,
        fonts,
        &level.objective.describe(),
        EditorButton::Objective,
        false,
    );
    panel.spawn(fonts.heading(tr("SIDES"), 15.0, TEXT_DIM));
    let faction = tr(Team::Player.name_in(level.player_faction));
    let faction_label = tr_fmt("Your faction: {}", &[&faction]);
    spawn_button(panel, fonts, &faction_label, EditorButton::Faction, false);
    let sides = std::iter::once((tr("You").to_string(), level.player.site, None)).chain(
        level.enemies.iter().enumerate().map(|(index, enemy)| {
            let name = tr_fmt("Computer {}", &[&(index + 1)]);
            (name, enemy.force.site, Some((index, enemy.difficulty)))
        }),
    );
    for (side, (name, site, difficulty)) in sides.enumerate() {
        row(panel, |row| {
            row.spawn(fonts.text(name, 15.0, Team::ALL[side].ui_color()));
            spawn_button(row, fonts, site.name(), EditorButton::MoveSide(side), false);
            if let Some((enemy, difficulty)) = difficulty {
                let label = tr_owned(&format!("{difficulty:?}"));
                spawn_button(row, fonts, &label, EditorButton::Difficulty(enemy), false);
            }
        });
    }
    row(panel, |row| {
        spawn_button(row, fonts, tr("+ COMPUTER"), EditorButton::AddEnemy, false);
        spawn_button(
            row,
            fonts,
            tr("- COMPUTER"),
            EditorButton::RemoveEnemy,
            false,
        );
    });
}

fn row(parent: &mut ChildSpawnerCommands, children: impl FnOnce(&mut ChildSpawnerCommands)) {
    parent
        .spawn(Node {
            column_gap: Val::Px(6.0),
            align_items: AlignItems::Center,
            ..default()
        })
        .with_children(children);
}

fn spawn_button(
    parent: &mut ChildSpawnerCommands,
    fonts: &UiFonts,
    label: &str,
    button: EditorButton,
    active: bool,
) {
    parent
        .spawn((
            button,
            Node {
                min_height: Val::Px(28.0),
                min_width: Val::Px(44.0),
                padding: UiRect::axes(Val::Px(10.0), Val::Px(4.0)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(if active { BUTTON_ACTIVE } else { BUTTON_IDLE }),
            BorderColor::all(if active { ACCENT } else { FRAME }),
        ))
        .with_children(|button| {
            button.spawn(fonts.heading(label, 15.0, TEXT_MAIN));
        })
        .observe(press_editor_button);
}

fn press_editor_button(
    click: On<PointerClick>,
    buttons: Query<&EditorButton>,
    mut level: ResMut<ActiveLevel>,
    mut editor: ResMut<MapEditor>,
    mut next_state: ResMut<NextState<GameState>>,
) {
    let Ok(&button) = buttons.get(click.entity) else {
        return;
    };
    if click.button != PointerButton::Primary {
        return;
    }
    let level = &mut level.0;
    let clicked_text = matches!(button, EditorButton::Type(_));
    if !clicked_text {
        editor.typing = None;
    }
    match button {
        EditorButton::Page(page) => editor.page = page,
        EditorButton::Brush(brush) => editor.brush = brush,
        EditorButton::Radius(radius) => editor.radius = radius,
        EditorButton::Mirror => editor.mirror = !editor.mirror,
        EditorButton::Type(target) => editor.typing = Some(target),
        EditorButton::Objective => cycle_objective(level),
        EditorButton::MoveSide(side) => move_side(level, side),
        EditorButton::Difficulty(enemy) => cycle_difficulty(level, enemy),
        EditorButton::AddEnemy => {
            add_enemy(level);
        }
        EditorButton::RemoveEnemy => {
            remove_enemy(level);
        }
        EditorButton::Faction => level.player_faction = level.player_faction.opposite(),
        EditorButton::PlayTest => next_state.set(GameState::Playing),
        EditorButton::Save => save_edited_level(&mut editor, level),
        EditorButton::Exit => next_state.set(GameState::MainMenu),
    }
    editor.panel_outdated = true;
}

/// While a text is being written the keyboard belongs to it: brush and camera keys see nothing.
pub fn type_level_text(
    mut typed: MessageReader<KeyboardInput>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut editor: ResMut<MapEditor>,
    mut level: ResMut<ActiveLevel>,
) {
    let Some(target) = editor.typing else {
        typed.clear();
        return;
    };
    for key in typed.read().filter(|key| key.state.is_pressed()) {
        match &key.logical_key {
            Key::Enter | Key::Escape => editor.typing = None,
            Key::Backspace => erase_from(&mut level.0, target),
            Key::Space => type_into(&mut level.0, target, " "),
            Key::Character(text) => type_into(&mut level.0, target, text),
            _ => {}
        }
        editor.panel_outdated = true;
    }
    keys.reset_all();
}

fn save_edited_level(editor: &mut MapEditor, level: &LevelDefinition) {
    let saved = match &editor.file {
        Some(path) => overwrite_level(path, level).map(|()| path.clone()),
        None => save_custom_level(&custom_levels_directory(), level),
    };
    editor.panel_outdated = true;
    editor.message = Some(match saved {
        Ok(path) => {
            let message = tr_fmt(
                "Saved as {} - it now appears under Custom levels.",
                &[&path.display()],
            );
            editor.file = Some(path);
            message
        }
        Err(error) => tr_fmt("Could not save the level: {}", &[&error]),
    });
}

fn overwrite_level(path: &std::path::Path, level: &LevelDefinition) -> std::io::Result<()> {
    let json = serde_json::to_string_pretty(level).map_err(std::io::Error::other)?;
    std::fs::write(path, json)
}

pub fn leave_on_escape(
    keys: Res<ButtonInput<KeyCode>>,
    mut next_state: ResMut<NextState<GameState>>,
) {
    if keys.just_pressed(KeyCode::Escape) {
        next_state.set(GameState::MainMenu);
    }
}

pub fn choose_brush_by_key(keys: Res<ButtonInput<KeyCode>>, mut editor: ResMut<MapEditor>) {
    if let Some(index) = BRUSH_KEYS.iter().position(|&key| keys.just_pressed(key)) {
        editor.brush = Brush::ALL[index];
        editor.panel_outdated = true;
    }
}

pub fn paint_under_cursor(
    mouse: Res<ButtonInput<MouseButton>>,
    cursor: Res<Cursor>,
    editor: Res<MapEditor>,
    mut level: ResMut<ActiveLevel>,
    mut painter: MapPainter,
) {
    let brush = if mouse.pressed(MouseButton::Left) {
        editor.brush
    } else if mouse.pressed(MouseButton::Right) {
        Brush::Clear
    } else {
        return;
    };
    if cursor.over_interface {
        return;
    }
    let Some(ground) = cursor.ground else {
        return;
    };
    let ground_of_level = Ground::of(&level);
    for cell in brush_cells(world_to_cell(ground), editor.radius, editor.mirror) {
        let edit = MapEdit { cell, brush };
        let unchanged = level.0.map_edits.iter().rev().find(|e| e.cell == cell) == Some(&edit);
        if unchanged || !painter.paint(ground_of_level, edit) {
            continue;
        }
        level.0.map_edits.retain(|earlier| earlier.cell != cell);
        level.0.map_edits.push(edit);
    }
}

fn brush_cells(center: IVec2, radius: i32, mirror: bool) -> Vec<IVec2> {
    let reach = radius as f32 + 0.5;
    let mut cells: Vec<IVec2> = cells_by_distance(center, radius)
        .filter(|&cell| (cell - center).as_vec2().length() <= reach)
        .collect();
    if mirror {
        let mirrored: Vec<IVec2> = cells.iter().map(|&cell| opposite_cell(cell)).collect();
        cells.extend(
            mirrored
                .into_iter()
                .filter(|cell| GameMap::in_bounds(*cell)),
        );
        cells.dedup();
    }
    cells
}

/// Rebuilds the ground once a stroke ends, so painted or cleared crystals get or lose their soil.
pub fn resoil_after_stroke(
    mut painted: Local<bool>,
    mouse: Res<ButtonInput<MouseButton>>,
    level: Res<ActiveLevel>,
    map: Res<GameMap>,
    mut meshes: ResMut<Assets<Mesh>>,
    grounds: Query<&Mesh3d, With<PlayfieldGround>>,
) {
    *painted |= level.is_changed();
    let stroke_ended = mouse.any_just_released([MouseButton::Left, MouseButton::Right]);
    if !stroke_ended || !*painted {
        return;
    }
    *painted = false;
    for ground in &grounds {
        let _ = meshes.insert(&ground.0, playfield_ground_mesh(level.0.landscape, &map));
    }
}

pub fn draw_brush_outline(cursor: Res<Cursor>, editor: Res<MapEditor>, mut gizmos: Gizmos) {
    if cursor.over_interface {
        return;
    }
    let Some(ground) = cursor.ground else {
        return;
    };
    let color = if editor.brush == Brush::Clear {
        Color::srgb(1.0, 0.4, 0.3)
    } else {
        ACCENT
    };
    for cell in brush_cells(world_to_cell(ground), editor.radius, editor.mirror) {
        let center = cell_center(cell) + Vec3::Y * 0.15;
        gizmos.rect(
            Isometry3d::new(center, Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)),
            Vec2::splat(1.8),
            color,
        );
    }
}

/// Rings where the bases stand, since nothing can be painted there.
pub fn mark_base_sites(level: Res<ActiveLevel>, mut gizmos: Gizmos) {
    let footprint = BuildingKind::CommandCenter.spec().footprint;
    let sides = std::iter::once((Team::Player, level.0.player.site)).chain(
        level
            .0
            .opponents()
            .map(|(team, enemy)| (team, enemy.force.site)),
    );
    for (team, site) in sides {
        let center = footprint_center(site.origin(), footprint) + Vec3::Y * 0.3;
        gizmos.circle(
            Isometry3d::new(center, Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)),
            6.0,
            team.ui_color(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mirrored_brush_also_paints_the_opposite_cells() {
        let center = IVec2::new(10, 12);
        let cells = brush_cells(center, 0, true);
        assert_eq!(cells, vec![center, opposite_cell(center)]);
        assert_eq!(brush_cells(center, 1, false).len(), 9);
    }
}
