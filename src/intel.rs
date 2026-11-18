use std::collections::HashMap;

use bevy::prelude::*;

use crate::catalog::Team;
use crate::components::{Building, Unit};
use crate::fog::{FogOfWar, Sight};
use crate::map::{BASE_SITES, cell_center};

const UPDATE_INTERVAL_SECS: f32 = 0.2;

/// What each side knows about the other. The Syndicate commanders are allies and share one view of the
/// map; the Coalition's view is the player's `FogOfWar`. Enemy structures stay known once spotted.
#[derive(Resource, Clone)]
pub struct Intel {
    pub syndicate_vision: FogOfWar,
    known_structures: [HashMap<Entity, Vec3>; 2],
}

impl Intel {
    pub fn new(fog_of_war: bool) -> Self {
        Self {
            syndicate_vision: FogOfWar::new(fog_of_war),
            known_structures: Default::default(),
        }
    }

    pub fn vision<'a>(&'a self, viewer: Team, player_fog: &'a FogOfWar) -> &'a FogOfWar {
        if viewer == Team::Player {
            player_fog
        } else {
            &self.syndicate_vision
        }
    }

    /// Last known positions of enemy structures that `viewer`'s side has spotted and not seen destroyed.
    pub fn known_enemy_structures(&self, viewer: Team) -> impl Iterator<Item = Vec3> + '_ {
        self.known_structures[side(viewer)].values().copied()
    }

    pub fn known_enemy_structure_entities(
        &self,
        viewer: Team,
    ) -> impl Iterator<Item = (Entity, Vec3)> + '_ {
        self.known_structures[side(viewer)]
            .iter()
            .map(|(&entity, &position)| (entity, position))
    }

    /// Where to look for an enemy nobody has found: the nearest base site never explored, or failing that
    /// the nearest one out of sight right now.
    pub fn base_site_to_search(
        &self,
        viewer: Team,
        player_fog: &FogOfWar,
        from: Vec3,
    ) -> Option<Vec3> {
        let vision = self.vision(viewer, player_fog);
        let sites: Vec<Vec3> = BASE_SITES
            .iter()
            .map(|&site| cell_center(site + IVec2::ONE))
            .collect();
        let nearest = |candidates: Vec<Vec3>| {
            candidates
                .into_iter()
                .min_by(|a, b| a.distance(from).total_cmp(&b.distance(from)))
        };
        let unexplored = sites
            .iter()
            .copied()
            .filter(|&site| !vision.is_explored(site))
            .collect();
        let out_of_sight = sites
            .iter()
            .copied()
            .filter(|&site| !vision.is_visible(site))
            .collect();
        nearest(unexplored).or_else(|| nearest(out_of_sight))
    }
}

impl Default for Intel {
    fn default() -> Self {
        Self::new(true)
    }
}

fn side(team: Team) -> usize {
    usize::from(team != Team::Player)
}

pub fn update_intel(
    time: Res<Time>,
    mut countdown: Local<f32>,
    mut intel: ResMut<Intel>,
    player_fog: Res<FogOfWar>,
    viewers: Query<(&Team, &Transform, &Sight), Or<(With<Unit>, With<Building>)>>,
    buildings: Query<(Entity, &Building, &Team, &Transform)>,
) {
    *countdown -= time.delta_secs();
    if *countdown > 0.0 {
        return;
    }
    *countdown = UPDATE_INTERVAL_SECS;
    let syndicate_eyes = viewers
        .iter()
        .filter(|(team, ..)| **team != Team::Player)
        .map(|(_, transform, sight)| (transform.translation, sight.0));
    intel.syndicate_vision.reveal_around(syndicate_eyes);

    let intel = &mut *intel;
    for known in &mut intel.known_structures {
        known.retain(|&entity, _| buildings.contains(entity));
    }
    for (entity, building, &team, transform) in &buildings {
        if building.kind.is_wall() {
            continue;
        }
        let position = transform.translation;
        for viewer in [Team::Player, Team::Crimson] {
            let spotted = team.is_hostile_to(viewer)
                && intel.vision(viewer, &player_fog).is_visible(position);
            if spotted {
                intel.known_structures[side(viewer)].insert(entity, position);
            }
        }
    }
}
