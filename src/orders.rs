use bevy::prelude::*;

use crate::camera::{Cursor, RtsCamera};
use crate::catalog::{BuildingKind, Diplomacy, Team, UnitKind};
use crate::combat::{TargetInfo, Targets};
use crate::components::*;
use crate::engineers::{EngineerJob, EngineerTask, job_for};
use crate::i18n::{tr, tr_fmt};
use crate::key_bindings::Action;
use crate::map::{GameMap, cells_by_distance, world_to_cell};
use crate::movement::order_move;
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
        .filter(|t| t.seen_by_player)
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
                **team == Team::Player && *is_unit && inside_box(transform.translation)
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
        && hit.team == Team::Player
    {
        commands.entity(hit.entity).insert(Selected);
    }
}

type CommandableUnit<'a> = (
    Entity,
    &'a Transform,
    Option<&'a mut Mobile>,
    Option<&'a mut AttackTarget>,
    Option<&'a mut Harvester>,
    Option<&'a mut Orders>,
);

#[derive(Clone, Copy, PartialEq, Eq)]
enum GroundOrder {
    Move,
    AttackMove,
    Patrol,
}

/// Right click on the ground. Ctrl attack-moves, Alt patrols, Shift queues behind current orders.
fn ground_order_from_keys(keys: &ButtonInput<KeyCode>) -> (GroundOrder, bool) {
    let order = if keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]) {
        GroundOrder::AttackMove
    } else if keys.any_pressed([KeyCode::AltLeft, KeyCode::AltRight]) {
        GroundOrder::Patrol
    } else {
        GroundOrder::Move
    };
    (
        order,
        keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]),
    )
}

fn produces_units(kind: BuildingKind) -> bool {
    matches!(
        kind,
        BuildingKind::Barracks | BuildingKind::Factory | BuildingKind::Helipad
    )
}

pub fn issue_orders(
    mut commands: Commands,
    mut cues: MessageWriter<SoundCue>,
    mouse: Res<ButtonInput<MouseButton>>,
    (keys, settings): (Res<ButtonInput<KeyCode>>, Res<GameSettings>),
    cursor: Res<Cursor>,
    placement: Res<Placement>,
    tool: Res<StructureTool>,
    targets: Res<Targets>,
    map: Res<GameMap>,
    mut marker: ResMut<OrderMarker>,
    mut units: Query<CommandableUnit, (With<Selected>, With<Unit>)>,
    buildings: Query<&Building>,
    structures: Query<(&Building, &Health)>,
    diplomacy: Res<Diplomacy>,
    kinds: Query<&Unit>,
    selected_buildings: Query<(Entity, &Building, &Team), With<Selected>>,
) {
    if settings.keys.just_pressed(&keys, Action::Stop) {
        for (_, _, mobile, attack, _, orders) in &mut units {
            if let Some(mut mobile) = mobile {
                mobile.stop();
            }
            if let Some(mut attack) = attack {
                attack.clear();
            }
            if let Some(mut orders) = orders {
                orders.clear();
            }
        }
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
    let goal_cell = world_to_cell(ground);
    let mut acknowledge = |marker: &mut OrderMarker, position: Vec3, kind: MarkerKind| {
        cues.write(SoundCue::interface(Sound::UnitAcknowledge));
        *marker = OrderMarker {
            position,
            remaining_secs: ORDER_MARKER_SECS,
            kind,
        };
    };

    if units.is_empty() {
        let mut rally_set = false;
        for (entity, building, team) in &selected_buildings {
            if *team == Team::Player && produces_units(building.kind) {
                commands.entity(entity).insert(RallyPoint(goal_cell));
                rally_set = true;
            }
        }
        if rally_set {
            acknowledge(&mut marker, ground, MarkerKind::Rally);
        }
        return;
    }

    if let Some(structure) = hovered.filter(|t| t.is_building)
        && let Ok((building, health)) = structures.get(structure.entity)
        && let Some(job) = job_for(
            *diplomacy,
            Team::Player,
            structure.team,
            building.kind,
            health,
        )
    {
        let engineers: Vec<Entity> = units
            .iter()
            .map(|(entity, ..)| entity)
            .filter(|&entity| {
                kinds
                    .get(entity)
                    .is_ok_and(|unit| unit.0 == UnitKind::Engineer)
            })
            .collect();
        for &engineer in &engineers {
            commands
                .entity(engineer)
                .insert(EngineerTask::new(structure.entity, job));
        }
        if !engineers.is_empty() && job == EngineerJob::Repair {
            acknowledge(&mut marker, structure.position, MarkerKind::Move);
            return;
        }
    }

    if let Some(enemy) = hovered.filter(|t| t.team.is_hostile_to(Team::Player)) {
        for (_, _, mobile, attack, _, orders) in &mut units {
            if let (Some(mut attack), Some(mut mobile)) = (attack, mobile) {
                attack.order(enemy.entity);
                mobile.ordered_move = false;
                if let Some(mut orders) = orders {
                    orders.clear();
                }
            }
        }
        acknowledge(&mut marker, enemy.position, MarkerKind::Attack);
        return;
    }

    let own_refinery = hovered.filter(|t| {
        t.team == Team::Player
            && buildings
                .get(t.entity)
                .is_ok_and(|b| b.kind == BuildingKind::Refinery)
    });
    let (ground_order, queue) = ground_order_from_keys(&keys);
    let mut formation = cells_by_distance(goal_cell, 6).filter(|&cell| map.is_passable(cell));
    for (entity, transform, mobile, attack, harvester, orders) in &mut units {
        let Some(mut mobile) = mobile else { continue };
        commands.entity(entity).remove::<EngineerTask>();
        let position = transform.translation;
        if let Some(mut harvester) = harvester {
            if let Some(refinery) = own_refinery {
                harvester.state = HarvestState::ReturningToRefinery(refinery.entity);
                mobile.stop();
                continue;
            }
            if map.crystal_at(goal_cell) > 0 {
                harvester.preferred_field = Some(goal_cell);
                harvester.state = HarvestState::SeekingField;
                continue;
            }
            harvester.state = HarvestState::Holding;
            if let Some(cell) = formation.next() {
                order_move(&mut mobile, &map, position, cell, true);
            }
            continue;
        }
        let Some(cell) = formation.next() else { break };
        let order = match ground_order {
            GroundOrder::Move => Order::Move(cell),
            GroundOrder::AttackMove => Order::AttackMove(cell),
            GroundOrder::Patrol => Order::Patrol {
                from: world_to_cell(position),
                to: cell,
            },
        };
        let Some(mut orders) = orders else { continue };
        if queue {
            orders.queue(order);
            continue;
        }
        orders.replace_with(order);
        mobile.stop();
        if let Some(mut attack) = attack {
            attack.clear();
        }
    }
    let kind = match ground_order {
        GroundOrder::Move => MarkerKind::Move,
        GroundOrder::AttackMove => MarkerKind::Attack,
        GroundOrder::Patrol => MarkerKind::Patrol,
    };
    acknowledge(&mut marker, ground, kind);
}
