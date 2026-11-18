use bevy::prelude::*;

use crate::catalog::{BuildingKind, Team};
use crate::components::{Building, Unit};
use crate::economy::Economies;
use crate::i18n::tr_fmt;
use crate::levels::{ActiveLevel, Objective};
use crate::match_flow::MatchClock;

#[derive(Resource, Default, PartialEq, Eq, Clone, Copy, Debug)]
pub enum GameOutcome {
    #[default]
    Ongoing,
    Victory,
    Defeat,
}

/// The player loses once nothing of theirs is left; how they win depends on the mission objective.
pub fn decide_outcome(
    mut outcome: ResMut<GameOutcome>,
    level: Res<ActiveLevel>,
    clock: Res<MatchClock>,
    economies: Res<Economies>,
    units: Query<&Team, With<Unit>>,
    buildings: Query<(&Team, &Building)>,
) {
    if *outcome != GameOutcome::Ongoing {
        return;
    }
    let standing = |building: &Building| !building.kind.is_wall();
    let player_alive = units.iter().any(|&t| t == Team::Player)
        || buildings
            .iter()
            .any(|(&t, building)| t == Team::Player && standing(building));
    if !player_alive {
        *outcome = GameOutcome::Defeat;
        return;
    }
    let hostile = |team: &Team| team.is_hostile_to(Team::Player);
    let achieved = match level.0.objective {
        Objective::DestroyAllEnemies => {
            !units.iter().any(hostile)
                && !buildings
                    .iter()
                    .any(|(team, building)| hostile(team) && standing(building))
        }
        Objective::DestroyCommandCenters => !buildings
            .iter()
            .any(|(team, building)| hostile(team) && building.kind == BuildingKind::CommandCenter),
        Objective::CaptureCommandCenter => buildings.iter().any(|(&team, building)| {
            team == Team::Player
                && building.kind == BuildingKind::CommandCenter
                && building.origin != level.0.player.site.origin()
        }),
        Objective::Survive { minutes } => clock.elapsed_secs >= minutes * 60.0,
        Objective::ReachCredits { credits } => economies.of(Team::Player).credits >= credits,
    };
    if achieved {
        *outcome = GameOutcome::Victory;
    }
}

/// One-line progress report for the interface, e.g. "Hold out for 8 minutes - 3:12 left".
pub fn objective_status(level: &ActiveLevel, clock: &MatchClock, economies: &Economies) -> String {
    let objective = level.0.objective;
    let progress = match objective {
        Objective::Survive { minutes } => {
            let left = (minutes * 60.0 - clock.elapsed_secs).max(0.0) as u32;
            tr_fmt(
                "  -  {} left",
                &[&format!("{}:{:02}", left / 60, left % 60)],
            )
        }
        Objective::ReachCredits { credits } => {
            format!("  -  ${} / ${credits}", economies.of(Team::Player).credits)
        }
        Objective::DestroyAllEnemies
        | Objective::DestroyCommandCenters
        | Objective::CaptureCommandCenter => String::new(),
    };
    format!("{}{progress}", objective.describe())
}
