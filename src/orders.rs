use bevy::prelude::*;

use crate::camera::{Cursor, RtsCamera};
use crate::catalog::{BuildingKind, Diplomacy, Team, UnitKind};
use crate::combat::{TargetInfo, Targets};
use crate::components::*;
use crate::determinism::SimId;
use crate::engineers::{EngineerJob, job_for};
use crate::i18n::{tr, tr_fmt};
use crate::key_bindings::Action;
use crate::map::world_to_cell;
use crate::player_commands::LocalTeam;
use crate::player_commands::{CommandIssuer, GroundOrder, PlayerCommand};
use crate::production::Placement;
use crate::settings::GameSettings;
use crate::sound::{Sound, SoundCue};
use crate::structure_tools::StructureTool;
use crate::ui::UiFonts;

const DRAG_THRESHOLD_PX: f32 = 8.0;
const SELECTION_CORNER_PX: f32 = 14.0;
const SELECTION_CORNER_THICKNESS_PX: f32 = 3.0;
const CLICK_SLACK: f32 = 0.6;
const ORDER_MARKER_SECS: f32 = 0.6;

#[derive(Resource, Default)]
pub struct DragSelection {
    start: Option<Vec2>,
}

#[derive(Component)]
pub struct SelectionBoxUi;

#[derive(Resource, Default)]
pub struct OrderMarker {
    pub position: Vec3,
    pub remaining_secs: f32,
    pub kind: MarkerKind,
}

/// What the last right click ordered, which decides the marker's colour.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum MarkerKind {
    #[default]
    Move,
    Attack,
    Patrol,
    Rally,
}

#[derive(Component)]
pub struct SelectionCount;

pub fn spawn_selection_box(mut commands: Commands, fonts: Res<UiFonts>) {
    let corner = |placement: Node, border: UiRect| {
        (
            Node {
                position_type: PositionType::Absolute,
                width: Val::Px(SELECTION_CORNER_PX),
                height: Val::Px(SELECTION_CORNER_PX),
                border,
                ..placement
            },
            BorderColor::all(Color::WHITE),
        )
    };
    let edge = Val::Px(-SELECTION_CORNER_THICKNESS_PX);
    let thick = Val::Px(SELECTION_CORNER_THICKNESS_PX);
    commands
        .spawn((
            SelectionBoxUi,
            Pickable::IGNORE,
            Node {
                position_type: PositionType::Absolute,
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BorderColor::all(Color::srgba(1.0, 1.0, 1.0, 0.55)),
            BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.05)),
            Visibility::Hidden,
        ))
        .with_children(|frame| {
            frame.spawn(corner(
                Node {
                    left: edge,
                    top: edge,
                    ..default()
                },
                UiRect {
                    left: thick,
                    top: thick,
                    ..default()
                },
            ));
            frame.spawn(corner(
                Node {
                    right: edge,
                    top: edge,
                    ..default()
                },
                UiRect {
                    right: thick,
                    top: thick,
                    ..default()
                },
            ));
            frame.spawn(corner(
                Node {
                    left: edge,
                    bottom: edge,
                    ..default()
                },
                UiRect {
                    left: thick,
                    bottom: thick,
                    ..default()
                },
            ));
            frame.spawn(corner(
                Node {
                    right: edge,
                    bottom: edge,
                    ..default()
                },
                UiRect {
                    right: thick,
                    bottom: thick,
                    ..default()
                },
            ));
            frame.spawn((
                SelectionCount,
                fonts.heading("", 15.0, Color::WHITE),
                TextShadow::default(),
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(0.0),
                    top: Val::Px(-22.0),
                    ..default()
                },
            ));
        });
}

fn target_under(targets: &Targets, point: Vec3) -> Option<&TargetInfo> {
    targets
        .iter()
        .filter(|t| t.seen_locally)
        .filter(|t| {
            let offset = (point - t.position).xz().abs();
            let reach = t.radius + CLICK_SLACK;
            if t.is_building {
                offset.max_element() <= t.radius
            } else {
                offset.length() <= reach
            }
        })
        .min_by(|a, b| {
            a.position
                .distance_squared(point)
                .total_cmp(&b.position.distance_squared(point))
        })
}

pub fn select_units(
    local: Res<LocalTeam>,
    mut commands: Commands,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    cursor: Res<Cursor>,
    placement: Res<Placement>,
    tool: Res<StructureTool>,
    targets: Res<Targets>,
    mut drag: ResMut<DragSelection>,
    cameras: Query<(&Camera, &GlobalTransform), With<RtsCamera>>,
    selectable: Query<(Entity, &Team, &Transform, Has<Unit>)>,
    selected: Query<Entity, With<Selected>>,
    mut selection_box: Query<(&mut Node, &mut Visibility), With<SelectionBoxUi>>,
    mut counts: Query<&mut Text, With<SelectionCount>>,
) {
    let Some(screen) = cursor.screen else { return };
    let pointer_busy = placement.active.is_some() || tool.0.is_some();
    if keys.just_pressed(KeyCode::Escape) && !pointer_busy {
        for entity in &selected {
            commands.entity(entity).remove::<Selected>();
        }
    }
    if pointer_busy {
        drag.start = None;
    } else if mouse.just_pressed(MouseButton::Left) && !cursor.over_interface {
        drag.start = Some(screen);
    }

    let Ok((mut node, mut visibility)) = selection_box.single_mut() else {
        return;
    };
    let Some(start) = drag.start else {
        *visibility = Visibility::Hidden;
        return;
    };
    let min = start.min(screen);
    let max = start.max(screen);
    let dragging = start.distance(screen) > DRAG_THRESHOLD_PX;
    *visibility = if dragging {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };
    node.left = Val::Px(min.x);
    node.top = Val::Px(min.y);
    node.width = Val::Px(max.x - min.x);
    node.height = Val::Px(max.y - min.y);

    let Ok((camera, camera_transform)) = cameras.single() else {
        return;
    };
    let inside_box = |position: Vec3| {
        camera
            .world_to_viewport(camera_transform, position)
            .is_ok_and(|projected| projected.cmpge(min).all() && projected.cmple(max).all())
    };
    let own_units_inside = |selectable: &Query<(Entity, &Team, &Transform, Has<Unit>)>| {
        selectable
            .iter()
            .filter(|(_, team, transform, is_unit)| {
                **team == local.0 && *is_unit && inside_box(transform.translation)
            })
            .map(|(entity, ..)| entity)
            .collect::<Vec<_>>()
    };
    if dragging {
        let count = own_units_inside(&selectable).len();
        let label = match count {
            0 => String::new(),
            1 => tr("1 unit").to_string(),
            n => tr_fmt("{} units", &[&n]),
        };
        for mut text in &mut counts {
            if text.0 != label {
                text.0 = label.clone();
            }
        }
    }

    if !mouse.just_released(MouseButton::Left) {
        return;
    }
    drag.start = None;
    *visibility = Visibility::Hidden;

    let additive = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    if !additive {
        for entity in &selected {
            commands.entity(entity).remove::<Selected>();
        }
    }
    if dragging {
        for entity in own_units_inside(&selectable) {
            commands.entity(entity).insert(Selected);
        }
    } else if let Some(hit) = cursor
        .ground
        .and_then(|ground| target_under(&targets, ground))
        && hit.team == local.0
    {
        commands.entity(hit.entity).insert(Selected);
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Shortcut {
    order: GroundOrder,
    queued: bool,
}

/// Right click on the ground. Ctrl attack-moves, Alt patrols, Shift queues behind current orders.
fn ground_order_from_keys(keys: &ButtonInput<KeyCode>) -> Shortcut {
    let order = if keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]) {
        GroundOrder::AttackMove
    } else if keys.any_pressed([KeyCode::AltLeft, KeyCode::AltRight]) {
        GroundOrder::Patrol
    } else {
        GroundOrder::Move
    };
    Shortcut {
        order,
        queued: keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]),
    }
}

fn produces_units(kind: BuildingKind) -> bool {
    matches!(
        kind,
        BuildingKind::Barracks | BuildingKind::Factory | BuildingKind::Helipad
    )
}

/// Turns the player's clicks and keys into commands; the simulation carries them out on a later tick.
pub fn issue_orders(
    mut cues: MessageWriter<SoundCue>,
    mut issuer: CommandIssuer,
    mouse: Res<ButtonInput<MouseButton>>,
    (keys, settings): (Res<ButtonInput<KeyCode>>, Res<GameSettings>),
    cursor: Res<Cursor>,
    placement: Res<Placement>,
    tool: Res<StructureTool>,
    targets: Res<Targets>,
    mut marker: ResMut<OrderMarker>,
    diplomacy: Res<Diplomacy>,
    selected_units: Query<(&SimId, &Unit, &Team), With<Selected>>,
    selected_buildings: Query<(&SimId, &Building, &Team), With<Selected>>,
    structures: Query<(&SimId, &Building, &Health)>,
    sim_ids: Query<&SimId>,
) {
    let team = issuer.team();
    let units: Vec<SimId> = selected_units
        .iter()
        .filter(|(_, _, owner)| **owner == team)
        .map(|(&id, ..)| id)
        .collect();
    if settings.keys.just_pressed(&keys, Action::Stop) && !units.is_empty() {
        issuer.issue(PlayerCommand::Stop {
            units: units.clone(),
        });
    }
    if placement.active.is_some()
        || tool.0.is_some()
        || cursor.over_interface
        || !mouse.just_pressed(MouseButton::Right)
    {
        return;
    }
    let Some(ground) = cursor.ground else { return };
    let hovered = target_under(&targets, ground).copied();
    let goal = world_to_cell(ground);
    let mut acknowledge = |marker: &mut OrderMarker, position: Vec3, kind: MarkerKind| {
        cues.write(SoundCue::interface(Sound::UnitAcknowledge));
        *marker = OrderMarker {
            position,
            remaining_secs: ORDER_MARKER_SECS,
            kind,
        };
    };

    if units.is_empty() {
        let producers: Vec<SimId> = selected_buildings
            .iter()
            .filter(|(_, building, owner)| **owner == team && produces_units(building.kind))
            .map(|(&id, ..)| id)
            .collect();
        if !producers.is_empty() {
            issuer.issue(PlayerCommand::SetRally {
                buildings: producers,
                cell: goal,
            });
            acknowledge(&mut marker, ground, MarkerKind::Rally);
        }
        return;
    }

    if let Some(structure) = hovered.filter(|t| t.is_building)
        && let Ok((&id, building, health)) = structures.get(structure.entity)
        && let Some(job) = job_for(*diplomacy, team, structure.team, building.kind, health)
    {
        let engineers: Vec<SimId> = selected_units
            .iter()
            .filter(|(_, unit, owner)| **owner == team && unit.0 == UnitKind::Engineer)
            .map(|(&id, ..)| id)
            .collect();
        if !engineers.is_empty() {
            issuer.issue(PlayerCommand::SendEngineers {
                units: engineers,
                structure: id,
            });
            if job == EngineerJob::Repair {
                acknowledge(&mut marker, structure.position, MarkerKind::Move);
                return;
            }
        }
    }

    if let Some(enemy) = hovered.filter(|t| diplomacy.hostile(t.team, team))
        && let Ok(&target) = sim_ids.get(enemy.entity)
    {
        issuer.issue(PlayerCommand::Attack { units, target });
        acknowledge(&mut marker, enemy.position, MarkerKind::Attack);
        return;
    }

    let refinery = hovered
        .filter(|t| t.team == team)
        .and_then(|t| structures.get(t.entity).ok())
        .filter(|(_, building, _)| building.kind == BuildingKind::Refinery)
        .map(|(&id, ..)| id);
    let shortcut = ground_order_from_keys(&keys);
    issuer.issue(PlayerCommand::Ground {
        units,
        goal,
        order: shortcut.order,
        queued: shortcut.queued,
        refinery,
    });
    let kind = match shortcut.order {
        GroundOrder::Move => MarkerKind::Move,
        GroundOrder::AttackMove => MarkerKind::Attack,
        GroundOrder::Patrol => MarkerKind::Patrol,
    };
    acknowledge(&mut marker, ground, kind);
}
