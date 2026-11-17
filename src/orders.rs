use bevy::prelude::*;

use crate::camera::{Cursor, RtsCamera};
use crate::catalog::{BuildingKind, Team};
use crate::combat::{TargetInfo, Targets};
use crate::components::*;
use crate::map::{GameMap, cells_by_distance, world_to_cell};
use crate::movement::order_move;
use crate::production::Placement;

const DRAG_THRESHOLD_PX: f32 = 8.0;
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
    pub hostile: bool,
}

pub fn spawn_selection_box(mut commands: Commands) {
    commands.spawn((
        SelectionBoxUi,
        Node {
            position_type: PositionType::Absolute,
            border: UiRect::all(Val::Px(1.5)),
            ..default()
        },
        BorderColor::all(Color::srgb(0.4, 1.0, 0.5)),
        BackgroundColor(Color::srgba(0.4, 1.0, 0.5, 0.08)),
        Visibility::Hidden,
    ));
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
    targets: Res<Targets>,
    mut drag: ResMut<DragSelection>,
    cameras: Query<(&Camera, &GlobalTransform), With<RtsCamera>>,
    selectable: Query<(Entity, &Team, &Transform, Has<Unit>)>,
    selected: Query<Entity, With<Selected>>,
    mut selection_box: Query<(&mut Node, &mut Visibility), With<SelectionBoxUi>>,
) {
    let Some(screen) = cursor.screen else { return };
    if keys.just_pressed(KeyCode::Escape) && placement.active.is_none() {
        for entity in &selected {
            commands.entity(entity).remove::<Selected>();
        }
    }
    if placement.active.is_some() {
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
        let Ok((camera, camera_transform)) = cameras.single() else {
            return;
        };
        for (entity, &team, transform, is_unit) in &selectable {
            if team != Team::Player || !is_unit {
                continue;
            }
            let Ok(projected) = camera.world_to_viewport(camera_transform, transform.translation)
            else {
                continue;
            };
            if projected.cmpge(min).all() && projected.cmple(max).all() {
                commands.entity(entity).insert(Selected);
            }
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
);

pub fn issue_orders(
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    cursor: Res<Cursor>,
    placement: Res<Placement>,
    targets: Res<Targets>,
    map: Res<GameMap>,
    mut marker: ResMut<OrderMarker>,
    mut units: Query<CommandableUnit, (With<Selected>, With<Unit>)>,
    buildings: Query<&Building>,
) {
    if keys.just_pressed(KeyCode::KeyX) {
        for (_, _, mobile, attack, _) in &mut units {
            if let Some(mut mobile) = mobile {
                mobile.stop();
            }
            if let Some(mut attack) = attack {
                attack.clear();
            }
        }
    }
    if placement.active.is_some()
        || cursor.over_interface
        || !mouse.just_pressed(MouseButton::Right)
    {
        return;
    }
    let Some(ground) = cursor.ground else { return };
    let hovered = target_under(&targets, ground).copied();
    let goal_cell = world_to_cell(ground);

    if let Some(enemy) = hovered.filter(|t| t.team.is_hostile_to(Team::Player)) {
        for (_, _, mobile, attack, _) in &mut units {
            if let (Some(mut attack), Some(mut mobile)) = (attack, mobile) {
                attack.order(enemy.entity);
                mobile.ordered_move = false;
            }
        }
        *marker = OrderMarker {
            position: enemy.position,
            remaining_secs: ORDER_MARKER_SECS,
            hostile: true,
        };
        return;
    }

    let own_refinery = hovered.filter(|t| {
        t.team == Team::Player
            && buildings
                .get(t.entity)
                .is_ok_and(|b| b.kind == BuildingKind::Refinery)
    });
    let mut formation = cells_by_distance(goal_cell, 6).filter(|&cell| map.is_passable(cell));
    for (_, transform, mobile, attack, harvester) in &mut units {
        let Some(mut mobile) = mobile else { continue };
        if let Some(mut attack) = attack {
            attack.clear();
        }
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
        }
        let Some(cell) = formation.next() else { break };
        order_move(&mut mobile, &map, position, cell, true);
    }
    *marker = OrderMarker {
        position: ground,
        remaining_secs: ORDER_MARKER_SECS,
        hostile: false,
    };
}
