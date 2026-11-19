use bevy::prelude::*;

use crate::assets::ModelLibrary;
use crate::catalog::Team;
use crate::components::Building;
use crate::map::GameMap;

/// The four neighbours a wall can join, as cell steps.
const NEIGHBOURS: [IVec2; 4] = [IVec2::X, IVec2::Y, IVec2::NEG_X, IVec2::NEG_Y];

/// Which neighbours a wall is joined to, one bit per entry of `NEIGHBOURS`.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct WallJoints(u8);

#[derive(Component)]
pub struct WallRun;

/// Joins every wall post to the wall posts of its own side next to it with a run of wall, so a line of
/// segments reads as one wall. Purely visual; only rechecked when a structure appears, falls or changes hands.
pub fn join_walls(
    mut commands: Commands,
    models: Res<ModelLibrary>,
    map: Res<GameMap>,
    mut removed: RemovedComponents<Building>,
    changed: Query<(), (With<Building>, Or<(Added<Building>, Changed<Team>)>)>,
    walls: Query<(
        Entity,
        &Building,
        &Team,
        &Transform,
        Option<&WallJoints>,
        Option<&Children>,
    )>,
    runs: Query<(), With<WallRun>>,
) {
    let anything_removed = removed.read().count() > 0;
    if !anything_removed && changed.is_empty() {
        return;
    }
    let wall_of = |entity: Entity| {
        walls
            .get(entity)
            .ok()
            .filter(|(_, building, ..)| building.kind.is_wall())
            .map(|(_, _, team, ..)| *team)
    };
    for (entity, building, &team, placement, joints, children) in &walls {
        if !building.kind.is_wall() {
            continue;
        }
        let wanted = joints_with(|step| {
            map.building_at(building.origin + step)
                .and_then(wall_of)
                .is_some_and(|neighbour| neighbour == team)
        });
        if joints.copied() == Some(wanted) {
            continue;
        }
        for &child in children.into_iter().flatten() {
            if runs.contains(child) {
                commands.entity(child).despawn();
            }
        }
        let own_yaw = placement.rotation.to_euler(EulerRot::YXZ).0;
        commands
            .entity(entity)
            .insert(wanted)
            .with_children(|post| {
                for step in wanted.steps() {
                    post.spawn((
                        WallRun,
                        WorldAssetRoot(models.wall_link(team)),
                        Transform::from_rotation(Quat::from_rotation_y(
                            yaw_towards(step) - own_yaw,
                        )),
                    ));
                }
            });
    }
}

fn joints_with(joined: impl Fn(IVec2) -> bool) -> WallJoints {
    WallJoints(
        NEIGHBOURS
            .iter()
            .enumerate()
            .filter(|&(_, &step)| joined(step))
            .fold(0, |bits, (index, _)| bits | 1 << index),
    )
}

impl WallJoints {
    fn steps(self) -> impl Iterator<Item = IVec2> {
        NEIGHBOURS
            .into_iter()
            .enumerate()
            .filter(move |&(index, _)| self.0 & 1 << index != 0)
            .map(|(_, step)| step)
    }
}

/// The yaw that turns the run model, which points along +X, towards a neighbouring cell; cell rows run along +Z.
fn yaw_towards(step: IVec2) -> f32 {
    (-step.y as f32).atan2(step.x as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_run_points_at_the_neighbour_it_joins() {
        for step in NEIGHBOURS {
            let pointing = Quat::from_rotation_y(yaw_towards(step)) * Vec3::X;
            let towards = Vec3::new(step.x as f32, 0.0, step.y as f32);
            assert!(pointing.distance(towards) < 1e-5, "{step}: {pointing}");
        }
    }

    #[test]
    fn joints_name_exactly_the_joined_neighbours() {
        let joints = joints_with(|step| step == IVec2::X || step == IVec2::NEG_Y);
        let steps: Vec<IVec2> = joints.steps().collect();
        assert_eq!(steps, vec![IVec2::X, IVec2::NEG_Y]);
        assert_eq!(joints_with(|_| false).steps().count(), 0);
    }
}
