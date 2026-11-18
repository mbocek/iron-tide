use std::collections::HashMap;

use bevy::prelude::*;

use crate::catalog::{Diplomacy, Team};
use crate::components::{Building, Unit};
use crate::fog::{FogOfWar, Sight};
use crate::map::{BASE_SITES, cell_center};

const UPDATE_INTERVAL_SECS: f32 = 0.2;

/// What each team knows. A computer opponent sees what its own and its allies' units and buildings see;
/// the Coalition's view is the player's `FogOfWar`. Enemy structures stay known once spotted.
#[derive(Resource, Clone)]
pub struct Intel {
    /// Indexed by `Team::index`; the player's slot is unused.
    visions: [FogOfWar; 4],
    known_structures: [HashMap<Entity, Vec3>; 4],
}

impl Intel {
    pub fn new(fog_of_war: bool) -> Self {
        Self {
            visions: std::array::from_fn(|_| FogOfWar::new(fog_of_war)),
            known_structures: Default::default(),
        }
    }

    pub fn vision<'a>(&'a self, viewer: Team, player_fog: &'a FogOfWar) -> &'a FogOfWar {
        if viewer == Team::Player {
            player_fog
        } else {
            &self.visions[viewer.index()]
        }
    }

    pub fn opponent_vision_mut(&mut self, opponent: Team) -> &mut FogOfWar {
        &mut self.visions[opponent.index()]
    }

    /// Last known positions of enemy structures that `viewer`'s side has spotted and not seen destroyed.
    pub fn known_enemy_structures(&self, viewer: Team) -> impl Iterator<Item = Vec3> + '_ {
        self.known_structures[viewer.index()].values().copied()
    }

    pub fn known_enemy_structure_entities(
        &self,
        viewer: Team,
    ) -> impl Iterator<Item = (Entity, Vec3)> + '_ {
        self.known_structures[viewer.index()]
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

pub fn update_intel(
    time: Res<Time>,
    mut countdown: Local<f32>,
    mut intel: ResMut<Intel>,
    player_fog: Res<FogOfWar>,
    diplomacy: Res<Diplomacy>,
    viewers: Query<(&Team, &Transform, &Sight), Or<(With<Unit>, With<Building>)>>,
    buildings: Query<(Entity, &Building, &Team, &Transform)>,
) {
    *countdown -= time.delta_secs();
    if *countdown > 0.0 {
        return;
    }
    *countdown = UPDATE_INTERVAL_SECS;
    for opponent in Team::OPPONENTS {
        let allied_eyes = viewers
            .iter()
            .filter(|(team, ..)| !diplomacy.hostile(**team, opponent))
            .map(|(_, transform, sight)| (transform.translation, sight.0));
        intel
            .opponent_vision_mut(opponent)
            .reveal_around(allied_eyes);
    }

    let intel = &mut *intel;
    for known in &mut intel.known_structures {
        known.retain(|&entity, _| buildings.contains(entity));
    }
    for (entity, building, &team, transform) in &buildings {
        if building.kind.is_wall() {
            continue;
        }
        let position = transform.translation;
        for viewer in Team::ALL {
            let spotted = diplomacy.hostile(team, viewer)
                && intel.vision(viewer, &player_fog).is_visible(position);
            if spotted {
                intel.known_structures[viewer.index()].insert(entity, position);
            }
        }
    }
}
