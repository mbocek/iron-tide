use bevy::ecs::system::SystemParam;
use bevy::prelude::*;

use crate::catalog::{Diplomacy, Team};
use crate::fog::FogOfWar;
use crate::intel::Intel;
use crate::player_commands::LocalTeam;

/// The battle as this machine's player sees it. The simulation keeps every team's view; drawing, sound and
/// the interface must use only the local one, or a networked opponent would see through the fog.
#[derive(SystemParam)]
pub struct LocalView<'w> {
    fog: Res<'w, FogOfWar>,
    intel: Res<'w, Intel>,
    local: Res<'w, LocalTeam>,
    diplomacy: Res<'w, Diplomacy>,
}

impl LocalView<'_> {
    pub fn team(&self) -> Team {
        self.local.0
    }

    pub fn sight(&self) -> &FogOfWar {
        self.intel.vision(self.local.0, &self.fog)
    }

    pub fn is_hostile(&self, team: Team) -> bool {
        self.diplomacy.hostile(team, self.local.0)
    }

    /// Whether the local player may know about something of `team` standing at `position`.
    pub fn reveals(&self, team: Team, position: Vec3) -> bool {
        !self.is_hostile(team) || self.sight().is_visible(position)
    }
}
