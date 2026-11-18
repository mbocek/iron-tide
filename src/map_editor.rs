use std::path::PathBuf;

use bevy::prelude::*;

use crate::camera::Cursor;
use crate::catalog::{BuildingKind, Team};
use crate::i18n::{tr, tr_fmt};
use crate::levels::{ActiveLevel, LevelDefinition, custom_levels_directory, save_custom_level};
use crate::map::{
    GameMap, cell_center, cells_by_distance, footprint_center, opposite_cell, world_to_cell,
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
}

impl MapEditor {
    pub fn new(file: Option<PathBuf>) -> MapEditor {
        MapEditor {
            brush: Brush::Crystal,
            radius: 1,
            mirror: true,
            file,
            message: None,
        }
    }
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
    Brush(Brush),
    Radius(i32),
    Mirror,
    Save,
    Exit,
}

#[derive(Component)]
pub struct EditorMessage;

pub fn spawn_map_editor_panel(
    mut commands: Commands,
    fonts: Res<UiFonts>,
    level: Res<ActiveLevel>,
) {
    commands
        .spawn((
            DespawnOnExit(GameState::MapEditor),
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(0.0),
                right: Val::Px(0.0),
                bottom: Val::Px(0.0),
                width: Val::Px(SIDEBAR_WIDTH),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(14.0)),
                row_gap: Val::Px(8.0),
                border: UiRect::left(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(PANEL),
            BorderColor::all(FRAME),
        ))
        .with_children(|panel| {
            panel.spawn(fonts.heading(tr("MAP EDITOR"), 24.0, ACCENT));
            panel.spawn(fonts.text(level.0.name.clone(), 15.0, TEXT_DIM));
            panel.spawn(fonts.heading(tr("BRUSH"), 15.0, TEXT_DIM));
            for (index, brush) in Brush::ALL.into_iter().enumerate() {
                let label = format!("{}  {}", index + 1, tr(brush.name()));
                spawn_button(panel, &fonts, &label, EditorButton::Brush(brush));
            }
            panel.spawn(fonts.heading(tr("SIZE"), 15.0, TEXT_DIM));
            panel
                .spawn(Node {
                    column_gap: Val::Px(6.0),
                    ..default()
                })
                .with_children(|row| {
                    for radius in BRUSH_SIZES {
                        let label = (2 * radius + 1).to_string();
                        spawn_button(row, &fonts, &label, EditorButton::Radius(radius));
                    }
                });
            spawn_button(panel, &fonts, "", EditorButton::Mirror);
            panel.spawn(fonts.text(
                tr("Left button paints, right button clears. Bases stay open. Mirroring keeps both halves of the map equally fair."),
                14.0,
                TEXT_DIM,
            ));
            spawn_button(panel, &fonts, tr("SAVE"), EditorButton::Save);
            spawn_button(panel, &fonts, tr("BACK TO MENU"), EditorButton::Exit);
            panel.spawn((EditorMessage, fonts.text("", 14.0, GOOD)));
        });
}

fn spawn_button(
    parent: &mut ChildSpawnerCommands,
    fonts: &UiFonts,
    label: &str,
    button: EditorButton,
) {
    parent
        .spawn((
            button,
            Node {
                min_height: Val::Px(30.0),
                min_width: Val::Px(44.0),
                padding: UiRect::axes(Val::Px(10.0), Val::Px(4.0)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(BUTTON_IDLE),
            BorderColor::all(FRAME),
        ))
        .with_children(|button| {
            button.spawn(fonts.heading(label, 16.0, TEXT_MAIN));
        })
        .observe(press_editor_button);
}

fn press_editor_button(
    click: On<PointerClick>,
    buttons: Query<&EditorButton>,
    level: Res<ActiveLevel>,
    mut editor: ResMut<MapEditor>,
    mut next_state: ResMut<NextState<GameState>>,
) {
    let Ok(&button) = buttons.get(click.entity) else {
        return;
    };
    if click.button != PointerButton::Primary {
        return;
    }
    match button {
        EditorButton::Brush(brush) => editor.brush = brush,
        EditorButton::Radius(radius) => editor.radius = radius,
        EditorButton::Mirror => editor.mirror = !editor.mirror,
        EditorButton::Save => save_edited_level(&mut editor, &level.0),
        EditorButton::Exit => next_state.set(GameState::MainMenu),
    }
}

fn save_edited_level(editor: &mut MapEditor, level: &LevelDefinition) {
    let saved = match &editor.file {
        Some(path) => overwrite_level(path, level).map(|()| path.clone()),
        None => save_custom_level(&custom_levels_directory(), level),
    };
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

pub fn refresh_map_editor_panel(
    editor: Res<MapEditor>,
    buttons: Query<(&EditorButton, &Children)>,
    mut backgrounds: Query<(&EditorButton, &mut BackgroundColor)>,
    mut texts: Query<&mut Text, Without<EditorMessage>>,
    mut message: Query<&mut Text, With<EditorMessage>>,
) {
    for (button, mut background) in &mut backgrounds {
        let active = match *button {
            EditorButton::Brush(brush) => brush == editor.brush,
            EditorButton::Radius(radius) => radius == editor.radius,
            EditorButton::Mirror => editor.mirror,
            EditorButton::Save | EditorButton::Exit => false,
        };
        background.set_if_neq(BackgroundColor(if active {
            BUTTON_ACTIVE
        } else {
            BUTTON_IDLE
        }));
    }
    for (button, children) in &buttons {
        if !matches!(button, EditorButton::Mirror) {
            continue;
        }
        let label = if editor.mirror {
            tr("Mirror: on")
        } else {
            tr("Mirror: off")
        };
        for &child in children {
            if let Ok(mut text) = texts.get_mut(child) {
                text.set_if_neq(Text(label.to_string()));
            }
        }
    }
    if let Ok(mut text) = message.single_mut() {
        text.set_if_neq(Text(editor.message.clone().unwrap_or_default()));
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
