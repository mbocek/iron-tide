use std::path::{Path, PathBuf};

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::catalog::{BuildingKind, Team, UnitKind};
use crate::map::BASE_SITES;
use crate::settings::game_data_directory;
use crate::terrain::Landscape;

const CUSTOM_LEVELS_DIRECTORY: &str = "levels";
const CAMPAIGN_SOURCES: [&str; 4] = [
    include_str!("../assets/levels/01_bridgehead.json"),
    include_str!("../assets/levels/02_hold_the_ford.json"),
    include_str!("../assets/levels/03_iron_hammer.json"),
    include_str!("../assets/levels/04_three_fronts.json"),
];

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum BaseSite {
    SouthWest,
    NorthEast,
    NorthWest,
    SouthEast,
}

impl BaseSite {
    pub const ALL: [BaseSite; 4] = [
        BaseSite::SouthWest,
        BaseSite::NorthEast,
        BaseSite::NorthWest,
        BaseSite::SouthEast,
    ];

    pub fn origin(self) -> IVec2 {
        BASE_SITES[match self {
            BaseSite::SouthWest => 0,
            BaseSite::NorthEast => 1,
            BaseSite::NorthWest => 2,
            BaseSite::SouthEast => 3,
        }]
    }

    /// Which way units leave the base: towards the map centre.
    pub fn facing(self) -> f32 {
        match self {
            BaseSite::SouthWest | BaseSite::NorthWest => 0.0,
            BaseSite::NorthEast | BaseSite::SouthEast => std::f32::consts::PI,
        }
    }

    /// Direction (in cells) from the Command Center to where starting units gather.
    pub fn rally_offset(self) -> IVec2 {
        match self {
            BaseSite::SouthWest | BaseSite::NorthWest => IVec2::new(5, 1),
            BaseSite::NorthEast | BaseSite::SouthEast => IVec2::new(-3, 1),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Difficulty {
    Easy,
    Normal,
    Hard,
}

impl Difficulty {
    pub fn think_interval_secs(self) -> f32 {
        match self {
            Difficulty::Easy => 2.0,
            Difficulty::Normal => 1.0,
            Difficulty::Hard => 0.6,
        }
    }

    pub fn first_wave_size(self) -> usize {
        match self {
            Difficulty::Easy => 4,
            Difficulty::Normal => 6,
            Difficulty::Hard => 8,
        }
    }

    /// Multiplier on the credits a harvester delivers.
    pub fn income_rate(self) -> f32 {
        match self {
            Difficulty::Easy => 0.75,
            Difficulty::Normal => 1.0,
            Difficulty::Hard => 1.35,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub enum Objective {
    DestroyAllEnemies,
    DestroyCommandCenters,
    Survive { minutes: f32 },
    ReachCredits { credits: i32 },
}

impl Objective {
    pub fn describe(self) -> String {
        match self {
            Objective::DestroyAllEnemies => "Destroy every enemy unit and structure".into(),
            Objective::DestroyCommandCenters => "Destroy all enemy Command Centers".into(),
            Objective::Survive { minutes } => format!("Hold out for {minutes:.0} minutes"),
            Objective::ReachCredits { credits } => format!("Stockpile ${credits}"),
        }
    }
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct ForceSetup {
    pub site: BaseSite,
    pub credits: i32,
    /// Structures besides the Command Center, placed automatically around it.
    pub buildings: Vec<BuildingKind>,
    pub units: Vec<UnitKind>,
}

impl Default for ForceSetup {
    fn default() -> Self {
        Self {
            site: BaseSite::SouthWest,
            credits: 5000,
            buildings: Vec::new(),
            units: vec![
                UnitKind::Rifleman,
                UnitKind::Rifleman,
                UnitKind::Rifleman,
                UnitKind::Scout,
                UnitKind::Tank,
            ],
        }
    }
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct EnemySetup {
    #[serde(flatten)]
    pub force: ForceSetup,
    pub difficulty: Difficulty,
}

impl Default for EnemySetup {
    fn default() -> Self {
        Self {
            force: ForceSetup {
                site: BaseSite::NorthEast,
                ..default()
            },
            difficulty: Difficulty::Normal,
        }
    }
}

/// Everything that makes one mission: who fights where, with what, and how it is won.
#[derive(Resource, Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct LevelDefinition {
    pub name: String,
    pub briefing: String,
    /// Picks the variant of forests, rocks and crystal richness; the landscape itself is shared.
    pub map_seed: u64,
    pub player: ForceSetup,
    pub enemies: Vec<EnemySetup>,
    pub objective: Objective,
    /// Units and structures the mission allows; empty means everything.
    pub available_units: Vec<UnitKind>,
    pub available_buildings: Vec<BuildingKind>,
    /// Hide unexplored ground and unseen enemies.
    pub fog_of_war: bool,
    /// Walk the player through the basics step by step.
    pub tutorial: bool,
    /// The opponents fight each other as well as the player.
    pub free_for_all: bool,
    pub landscape: Landscape,
}

impl Default for LevelDefinition {
    fn default() -> Self {
        Self {
            name: "Skirmish".into(),
            briefing: "Destroy the Syndicate base.".into(),
            map_seed: 0x1a0_7de,
            player: ForceSetup::default(),
            enemies: vec![EnemySetup::default()],
            objective: Objective::DestroyAllEnemies,
            available_units: Vec::new(),
            available_buildings: Vec::new(),
            fog_of_war: true,
            tutorial: false,
            free_for_all: false,
            landscape: Landscape::River,
        }
    }
}

impl LevelDefinition {
    /// Opponents in team order; a level lists at most three.
    pub fn opponents(&self) -> impl Iterator<Item = (Team, &EnemySetup)> {
        Team::OPPONENTS.into_iter().zip(self.enemies.iter())
    }

    pub fn force_of(&self, team: Team) -> Option<&ForceSetup> {
        if team == Team::Player {
            return Some(&self.player);
        }
        self.opponents()
            .find(|(t, _)| *t == team)
            .map(|(_, enemy)| &enemy.force)
    }

    pub fn allows_unit(&self, kind: UnitKind) -> bool {
        self.available_units.is_empty() || self.available_units.contains(&kind)
    }

    pub fn allows_building(&self, kind: BuildingKind) -> bool {
        kind == BuildingKind::CommandCenter
            || self.available_buildings.is_empty()
            || self.available_buildings.contains(&kind)
    }

    /// Problems that would make the level unplayable, in plain words; empty when it is fine.
    pub fn problems(&self) -> Vec<String> {
        let mut problems = Vec::new();
        if self.enemies.is_empty() || self.enemies.len() > Team::OPPONENTS.len() {
            problems.push(format!("needs 1 to {} enemies", Team::OPPONENTS.len()));
        }
        let mut sites = vec![self.player.site];
        for (_, enemy) in self.opponents() {
            if sites.contains(&enemy.force.site) {
                problems.push(format!("two sides start at {:?}", enemy.force.site));
            }
            sites.push(enemy.force.site);
        }
        if self.player.credits < 0 || self.enemies.iter().any(|enemy| enemy.force.credits < 0) {
            problems.push("credits cannot be negative".into());
        }
        problems
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum LevelSource {
    Campaign(usize),
    Custom(PathBuf),
    Skirmish,
}

#[derive(Clone, Debug)]
pub struct LevelEntry {
    pub source: LevelSource,
    pub level: LevelDefinition,
}

/// The level the next match is built from.
#[derive(Resource, Clone, Debug, Default)]
pub struct ActiveLevel(pub LevelDefinition);

pub fn campaign() -> Vec<LevelEntry> {
    CAMPAIGN_SOURCES
        .iter()
        .enumerate()
        .map(|(index, json)| LevelEntry {
            source: LevelSource::Campaign(index),
            level: serde_json::from_str(json)
                .unwrap_or_else(|error| panic!("campaign level {index} is invalid: {error}")),
        })
        .collect()
}

pub fn custom_levels_directory() -> PathBuf {
    game_data_directory().join(CUSTOM_LEVELS_DIRECTORY)
}

/// Every readable, playable level in the custom levels directory, sorted by file name.
pub fn custom_levels(directory: &Path) -> Vec<LevelEntry> {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect();
    paths.sort();
    paths
        .into_iter()
        .filter_map(|path| {
            let level: LevelDefinition =
                serde_json::from_str(&std::fs::read_to_string(&path).ok()?).ok()?;
            level.problems().is_empty().then_some(LevelEntry {
                source: LevelSource::Custom(path),
                level,
            })
        })
        .collect()
}

/// Writes the level as a new JSON file and returns its path.
pub fn save_custom_level(directory: &Path, level: &LevelDefinition) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(directory)?;
    let stem: String = level
        .name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    let path = (1..)
        .map(|n| directory.join(format!("{stem}_{n}.json")))
        .find(|candidate| !candidate.exists())
        .expect("an unused file name exists");
    let json = serde_json::to_string_pretty(level).map_err(std::io::Error::other)?;
    std::fs::write(&path, json)?;
    Ok(path)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TechLevel {
    Basic,
    Standard,
    Full,
}

impl TechLevel {
    pub fn label(self) -> &'static str {
        match self {
            TechLevel::Basic => "Basic (no heavy units)",
            TechLevel::Standard => "Standard (no Bulwark)",
            TechLevel::Full => "Full arsenal",
        }
    }

    fn units(self) -> Vec<UnitKind> {
        match self {
            TechLevel::Basic => vec![
                UnitKind::Rifleman,
                UnitKind::Scout,
                UnitKind::Tank,
                UnitKind::Harvester,
            ],
            TechLevel::Standard => UnitKind::ALL
                .into_iter()
                .filter(|&k| k != UnitKind::Bulwark)
                .collect(),
            TechLevel::Full => Vec::new(),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ObjectiveKind {
    Annihilate,
    Decapitate,
    Survive,
    Economy,
}

/// The choices of the quick-battle editor, turned into a level on demand.
#[derive(Resource, Clone, Debug, PartialEq)]
pub struct SkirmishOptions {
    pub enemies: usize,
    pub difficulty: Difficulty,
    pub credits: i32,
    pub objective: ObjectiveKind,
    pub tech: TechLevel,
    pub map_variant: u64,
    pub player_site: BaseSite,
    pub fog_of_war: bool,
    pub free_for_all: bool,
    pub landscape: Landscape,
}

impl Default for SkirmishOptions {
    fn default() -> Self {
        Self {
            enemies: 1,
            difficulty: Difficulty::Normal,
            credits: 5000,
            objective: ObjectiveKind::Annihilate,
            tech: TechLevel::Full,
            map_variant: 1,
            player_site: BaseSite::SouthWest,
            fog_of_war: true,
            free_for_all: false,
            landscape: Landscape::River,
        }
    }
}

impl SkirmishOptions {
    pub fn to_level(&self) -> LevelDefinition {
        let enemy_sites = BaseSite::ALL
            .into_iter()
            .filter(|&site| site != self.player_site);
        let enemies = enemy_sites
            .take(self.enemies.clamp(1, Team::OPPONENTS.len()))
            .map(|site| EnemySetup {
                force: ForceSetup {
                    site,
                    credits: self.credits,
                    ..default()
                },
                difficulty: self.difficulty,
            })
            .collect();
        let objective = match self.objective {
            ObjectiveKind::Annihilate => Objective::DestroyAllEnemies,
            ObjectiveKind::Decapitate => Objective::DestroyCommandCenters,
            ObjectiveKind::Survive => Objective::Survive { minutes: 10.0 },
            ObjectiveKind::Economy => Objective::ReachCredits {
                credits: self.credits + 10_000,
            },
        };
        LevelDefinition {
            name: format!("Battle vs {} ({:?})", self.enemies, self.difficulty),
            briefing: "A custom battle.".into(),
            map_seed: 0x1a0_7de + self.map_variant * 7919,
            player: ForceSetup {
                site: self.player_site,
                credits: self.credits,
                ..default()
            },
            enemies,
            objective,
            available_units: self.tech.units(),
            available_buildings: Vec::new(),
            fog_of_war: self.fog_of_war,
            tutorial: false,
            free_for_all: self.free_for_all,
            landscape: self.landscape,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_campaign_level_is_playable() {
        let missions = campaign();
        assert_eq!(missions.len(), CAMPAIGN_SOURCES.len());
        for mission in missions {
            assert!(
                mission.level.problems().is_empty(),
                "{}: {:?}",
                mission.level.name,
                mission.level.problems()
            );
            assert!(!mission.level.briefing.is_empty());
        }
    }

    #[test]
    fn skirmish_options_produce_valid_levels_for_every_enemy_count() {
        for enemies in 1..=3 {
            for player_site in BaseSite::ALL {
                let options = SkirmishOptions {
                    enemies,
                    player_site,
                    ..default()
                };
                let level = options.to_level();
                assert_eq!(level.enemies.len(), enemies);
                assert!(level.problems().is_empty(), "{:?}", level.problems());
            }
        }
    }

    #[test]
    fn broken_levels_are_reported() {
        let mut level = LevelDefinition::default();
        level.enemies[0].force.site = BaseSite::SouthWest;
        assert!(!level.problems().is_empty());
        level.enemies.clear();
        assert!(!level.problems().is_empty());
    }

    #[test]
    fn tech_restrictions_apply_to_units_but_never_to_the_command_center() {
        let level = LevelDefinition {
            available_units: vec![UnitKind::Rifleman],
            available_buildings: vec![BuildingKind::Barracks],
            ..default()
        };
        assert!(level.allows_unit(UnitKind::Rifleman) && !level.allows_unit(UnitKind::Tank));
        assert!(
            level.allows_building(BuildingKind::CommandCenter)
                && !level.allows_building(BuildingKind::Factory)
        );
    }

    #[test]
    fn custom_levels_round_trip_through_files() {
        let directory =
            std::env::temp_dir().join(format!("iron_tide_levels_test_{}", std::process::id()));
        let level = SkirmishOptions {
            enemies: 2,
            ..default()
        }
        .to_level();
        let path = save_custom_level(&directory, &level).unwrap();
        std::fs::write(directory.join("broken.json"), "{ not json").unwrap();
        let loaded = custom_levels(&directory);
        assert_eq!(loaded.len(), 1, "broken files are skipped");
        assert_eq!(loaded[0].level, level);
        assert_eq!(loaded[0].source, LevelSource::Custom(path));
        std::fs::remove_dir_all(&directory).unwrap();
    }
}
