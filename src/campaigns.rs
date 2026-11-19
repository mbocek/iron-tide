use bevy::prelude::*;

use crate::catalog::Faction;
use crate::levels::{Difficulty, LevelDefinition, campaign};

/// A chain of missions played in order, each unlocked by winning the one before.
#[derive(Clone, Debug)]
pub struct Campaign {
    /// Stable name for remembering progress; the built-in campaigns keep the bare mission names they always had.
    pub key: String,
    pub title: String,
    pub faction: Faction,
    pub missions: Vec<LevelDefinition>,
    built_in: bool,
}

/// Every campaign the player can play: the built-in ones and any custom ones found on disk.
#[derive(Resource, Clone, Debug)]
pub struct CampaignLibrary(pub Vec<Campaign>);

impl Default for CampaignLibrary {
    fn default() -> Self {
        Self(built_in_campaigns())
    }
}

/// The campaign mission being played, so a victory can be remembered and the next mission offered.
#[derive(Resource, Clone, Debug)]
pub struct CampaignRun {
    pub key: String,
    pub mission: usize,
    pub difficulty: Difficulty,
}

impl Campaign {
    /// Where a win of `mission` is remembered among the completed missions.
    pub fn progress_key(&self, mission: usize) -> String {
        let name = &self.missions[mission].name;
        if self.built_in {
            name.clone()
        } else {
            format!("{}/{}", self.key, name)
        }
    }

    pub fn won(&self, mission: usize, completed: &[String]) -> bool {
        completed.contains(&self.progress_key(mission))
    }

    pub fn unlocked(&self, mission: usize, completed: &[String]) -> bool {
        mission == 0 || self.won(mission - 1, completed)
    }

    pub fn won_count(&self, completed: &[String]) -> usize {
        (0..self.missions.len())
            .filter(|&mission| self.won(mission, completed))
            .count()
    }

    /// The first unlocked mission not won yet; `None` once the campaign is finished.
    pub fn next_to_play(&self, completed: &[String]) -> Option<usize> {
        (0..self.missions.len())
            .find(|&mission| !self.won(mission, completed) && self.unlocked(mission, completed))
    }

    /// The mission as it is played at the chosen campaign difficulty.
    pub fn mission_at(&self, mission: usize, difficulty: Difficulty) -> LevelDefinition {
        at_difficulty(&self.missions[mission], difficulty)
    }
}

/// The two built-in campaigns: the Coalition's missions and the Syndicate's, in their authored order.
pub fn built_in_campaigns() -> Vec<Campaign> {
    let missions: Vec<LevelDefinition> = campaign().into_iter().map(|entry| entry.level).collect();
    [
        ("coalition", Faction::Coalition),
        ("syndicate", Faction::Syndicate),
    ]
    .into_iter()
    .map(|(key, faction)| Campaign {
        key: key.to_string(),
        title: faction.title().to_string(),
        faction,
        missions: missions
            .iter()
            .filter(|level| level.player_faction == faction)
            .cloned()
            .collect(),
        built_in: true,
    })
    .collect()
}

pub fn find_campaign<'a>(campaigns: &'a [Campaign], key: &str) -> Option<&'a Campaign> {
    campaigns.iter().find(|campaign| campaign.key == key)
}

/// Shifts every computer commander of a mission from its authored difficulty by how far the chosen campaign
/// difficulty is from Normal: the first missions stay gentle on Easy and every mission gets harder on Brutal.
pub fn at_difficulty(level: &LevelDefinition, chosen: Difficulty) -> LevelDefinition {
    let rank = |difficulty: Difficulty| {
        Difficulty::ALL
            .iter()
            .position(|&d| d == difficulty)
            .unwrap_or(1) as i32
    };
    let shift = rank(chosen) - rank(Difficulty::Normal);
    let mut level = level.clone();
    for enemy in &mut level.enemies {
        let shifted = (rank(enemy.difficulty) + shift).clamp(0, Difficulty::ALL.len() as i32 - 1);
        enemy.difficulty = Difficulty::ALL[shifted as usize];
    }
    level
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_built_in_missions_split_into_a_coalition_and_a_syndicate_campaign() {
        let campaigns = built_in_campaigns();
        assert_eq!(campaigns.len(), 2);
        assert_eq!(campaigns[0].missions.len(), 10);
        assert_eq!(campaigns[1].missions.len(), 6);
        assert!(
            campaigns[1]
                .missions
                .iter()
                .all(|level| level.player_faction == Faction::Syndicate)
        );
    }

    #[test]
    fn a_mission_unlocks_once_the_one_before_is_won() {
        let coalition = &built_in_campaigns()[0];
        let none: Vec<String> = Vec::new();
        assert!(coalition.unlocked(0, &none));
        assert!(!coalition.unlocked(1, &none));
        assert_eq!(coalition.next_to_play(&none), Some(0));
        let first_won = vec![coalition.progress_key(0)];
        assert!(coalition.unlocked(1, &first_won));
        assert_eq!(coalition.next_to_play(&first_won), Some(1));
    }

    #[test]
    fn campaign_difficulty_shifts_every_commander_and_stays_within_the_levels() {
        let coalition = &built_in_campaigns()[0];
        let authored = &coalition.missions[0];
        let easy = coalition.mission_at(0, Difficulty::Easy);
        let brutal = coalition.mission_at(0, Difficulty::Brutal);
        for ((original, softer), harder) in authored
            .enemies
            .iter()
            .zip(&easy.enemies)
            .zip(&brutal.enemies)
        {
            assert!(softer.difficulty as usize <= original.difficulty as usize);
            assert!(harder.difficulty as usize >= original.difficulty as usize);
        }
    }
}
