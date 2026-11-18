use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::sound::Sound;

/// The player plus up to three computer opponents, who are allied with each other against the player.
#[derive(Component, Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub enum Team {
    Player,
    Crimson,
    Amber,
    Violet,
}

impl Team {
    pub const ALL: [Team; 4] = [Team::Player, Team::Crimson, Team::Amber, Team::Violet];
    pub const OPPONENTS: [Team; 3] = [Team::Crimson, Team::Amber, Team::Violet];

    pub fn index(self) -> usize {
        match self {
            Team::Player => 0,
            Team::Crimson => 1,
            Team::Amber => 2,
            Team::Violet => 3,
        }
    }

    /// The name in the usual line-up, where the player leads the Coalition.
    pub fn name(self) -> &'static str {
        self.name_in(Faction::default_of(self))
    }

    pub fn name_in(self, faction: Faction) -> &'static str {
        match (self, faction) {
            (Team::Player, Faction::Coalition) => "Coalition",
            (Team::Player, Faction::Syndicate) => "Syndicate",
            (Team::Crimson, Faction::Syndicate) => "Crimson Syndicate",
            (Team::Amber, Faction::Syndicate) => "Amber Syndicate",
            (Team::Violet, Faction::Syndicate) => "Violet Syndicate",
            (Team::Crimson, Faction::Coalition) => "Crimson Coalition",
            (Team::Amber, Faction::Coalition) => "Amber Coalition",
            (Team::Violet, Faction::Coalition) => "Violet Coalition",
        }
    }

    pub fn model_variant(self) -> &'static str {
        match self {
            Team::Player => "blue",
            Team::Crimson => "red",
            Team::Amber => "orange",
            Team::Violet => "purple",
        }
    }

    pub fn ui_color(self) -> Color {
        match self {
            Team::Player => Color::srgb(0.25, 0.55, 1.0),
            Team::Crimson => Color::srgb(1.0, 0.3, 0.25),
            Team::Amber => Color::srgb(1.0, 0.62, 0.15),
            Team::Violet => Color::srgb(0.72, 0.4, 1.0),
        }
    }
}

/// Whose armoury a team builds from; each side has units and structures the other cannot order.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum Faction {
    #[default]
    Coalition,
    Syndicate,
}

impl Faction {
    pub fn opposite(self) -> Faction {
        match self {
            Faction::Coalition => Faction::Syndicate,
            Faction::Syndicate => Faction::Coalition,
        }
    }

    /// The player leads the level's chosen faction and every computer side the other one.
    pub fn of(team: Team, player_faction: Faction) -> Faction {
        if team == Team::Player {
            player_faction
        } else {
            player_faction.opposite()
        }
    }

    fn default_of(team: Team) -> Faction {
        Faction::of(team, Faction::default())
    }

    pub fn title(self) -> &'static str {
        match self {
            Faction::Coalition => "COALITION",
            Faction::Syndicate => "SYNDICATE",
        }
    }
}

/// Which teams fight on the same side.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum Alliance {
    /// The Syndicate commanders are allies against the Coalition.
    #[default]
    CoalitionAgainstSyndicate,
    FreeForAll,
    /// The Coalition and Amber against Crimson and Violet: with the default sites, the two western bases
    /// against the two eastern ones, each pair the mirror image of the other.
    TwoAgainstTwo,
}

impl Alliance {
    pub const ALL: [Alliance; 3] = [
        Alliance::CoalitionAgainstSyndicate,
        Alliance::FreeForAll,
        Alliance::TwoAgainstTwo,
    ];

    fn side(self, team: Team) -> usize {
        match self {
            Alliance::CoalitionAgainstSyndicate => usize::from(team != Team::Player),
            Alliance::FreeForAll => team.index(),
            Alliance::TwoAgainstTwo => usize::from(matches!(team, Team::Crimson | Team::Violet)),
        }
    }
}

/// Who fights whom in the current match.
#[derive(Resource, Clone, Copy, PartialEq, Eq, Default, Debug)]
pub struct Diplomacy {
    pub alliance: Alliance,
}

impl Diplomacy {
    pub fn hostile(self, team: Team, other: Team) -> bool {
        self.alliance.side(team) != self.alliance.side(other)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ArmorClass {
    Infantry,
    Vehicle,
    Structure,
    Aircraft,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProjectileStyle {
    Bullet,
    Shell,
    ArtilleryShell,
    Missile,
    Flame,
}

#[derive(Clone, Copy, Debug)]
pub struct WeaponSpec {
    pub range: f32,
    pub damage: f32,
    pub reload_secs: f32,
    pub projectile_speed: f32,
    pub splash_radius: f32,
    pub style: ProjectileStyle,
    pub versus_infantry: f32,
    pub versus_vehicle: f32,
    pub versus_structure: f32,
    /// Zero for weapons that cannot shoot upwards at aircraft at all.
    pub versus_aircraft: f32,
    /// What firing it sounds like.
    pub report: Sound,
}

impl WeaponSpec {
    pub fn multiplier_against(&self, armor: ArmorClass) -> f32 {
        match armor {
            ArmorClass::Infantry => self.versus_infantry,
            ArmorClass::Vehicle => self.versus_vehicle,
            ArmorClass::Structure => self.versus_structure,
            ArmorClass::Aircraft => self.versus_aircraft,
        }
    }

    pub fn can_hit(&self, armor: ArmorClass) -> bool {
        self.multiplier_against(armor) > 0.0
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub enum UnitKind {
    Rifleman,
    Rocketeer,
    Medic,
    Engineer,
    Scout,
    Tank,
    Bulwark,
    Artillery,
    Harvester,
    Gunship,
    FlakTrack,
    Sniper,
    FlameTrooper,
    MissileTruck,
}

pub struct UnitSpec {
    pub name: &'static str,
    pub role: &'static str,
    pub model_id: &'static str,
    pub cost: i32,
    pub build_secs: f32,
    pub max_health: f32,
    pub speed: f32,
    pub turn_rate: f32,
    pub radius: f32,
    pub armor: ArmorClass,
    pub weapon: Option<WeaponSpec>,
    pub has_turret: bool,
    pub producer: BuildingKind,
    /// A structure needed besides the producer before the unit can be ordered.
    pub requires: Option<BuildingKind>,
    /// None when both factions can order it.
    pub faction: Option<Faction>,
    /// Health restored per second to wounded friendly infantry nearby; zero for everyone but medics.
    pub heal_per_sec: f32,
    /// Radius in meters this unit reveals through the fog of war.
    pub sight: f32,
}

impl UnitKind {
    pub const INFANTRY: [UnitKind; 6] = [
        UnitKind::Rifleman,
        UnitKind::Rocketeer,
        UnitKind::Sniper,
        UnitKind::FlameTrooper,
        UnitKind::Medic,
        UnitKind::Engineer,
    ];
    pub const VEHICLES: [UnitKind; 8] = [
        UnitKind::Scout,
        UnitKind::Tank,
        UnitKind::Bulwark,
        UnitKind::Artillery,
        UnitKind::MissileTruck,
        UnitKind::FlakTrack,
        UnitKind::Gunship,
        UnitKind::Harvester,
    ];
    pub const ALL: [UnitKind; 14] = [
        UnitKind::Rifleman,
        UnitKind::Rocketeer,
        UnitKind::Medic,
        UnitKind::Engineer,
        UnitKind::Scout,
        UnitKind::Tank,
        UnitKind::Bulwark,
        UnitKind::Artillery,
        UnitKind::Harvester,
        UnitKind::Gunship,
        UnitKind::FlakTrack,
        UnitKind::Sniper,
        UnitKind::FlameTrooper,
        UnitKind::MissileTruck,
    ];

    pub fn flies(self) -> bool {
        self.spec().armor == ArmorClass::Aircraft
    }

    pub fn spec(self) -> &'static UnitSpec {
        match self {
            UnitKind::Rifleman => &RIFLEMAN,
            UnitKind::Scout => &SCOUT,
            UnitKind::Tank => &TANK,
            UnitKind::Artillery => &ARTILLERY,
            UnitKind::Harvester => &HARVESTER,
            UnitKind::Gunship => &GUNSHIP,
            UnitKind::Rocketeer => &ROCKETEER,
            UnitKind::Medic => &MEDIC,
            UnitKind::Engineer => &ENGINEER,
            UnitKind::Bulwark => &BULWARK,
            UnitKind::FlakTrack => &FLAK_TRACK,
            UnitKind::Sniper => &SNIPER,
            UnitKind::FlameTrooper => &FLAME_TROOPER,
            UnitKind::MissileTruck => &MISSILE_TRUCK,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub enum BuildingKind {
    CommandCenter,
    PowerPlant,
    Refinery,
    Barracks,
    Factory,
    GunTurret,
    Wall,
    Helipad,
    FlakCannon,
    TechCenter,
    RepairBay,
    RocketBunker,
}

pub struct BuildingSpec {
    pub name: &'static str,
    pub role: &'static str,
    pub model_id: &'static str,
    pub footprint: IVec2,
    pub cost: i32,
    pub build_secs: f32,
    pub max_health: f32,
    pub power: i32,
    pub prerequisites: &'static [BuildingKind],
    pub weapon: Option<WeaponSpec>,
    /// Radius in meters this structure reveals through the fog of war.
    pub sight: f32,
    /// None when both factions can build it.
    pub faction: Option<Faction>,
}

impl BuildingKind {
    pub const CONSTRUCTIBLE: [BuildingKind; 11] = [
        BuildingKind::PowerPlant,
        BuildingKind::Refinery,
        BuildingKind::Barracks,
        BuildingKind::Factory,
        BuildingKind::TechCenter,
        BuildingKind::RepairBay,
        BuildingKind::Helipad,
        BuildingKind::GunTurret,
        BuildingKind::FlakCannon,
        BuildingKind::RocketBunker,
        BuildingKind::Wall,
    ];

    /// Walls are bought segment by segment while being placed, and do not keep a defeated side alive.
    pub fn is_wall(self) -> bool {
        self == BuildingKind::Wall
    }

    pub fn spec(self) -> &'static BuildingSpec {
        match self {
            BuildingKind::CommandCenter => &COMMAND_CENTER,
            BuildingKind::PowerPlant => &POWER_PLANT,
            BuildingKind::Refinery => &REFINERY,
            BuildingKind::Barracks => &BARRACKS,
            BuildingKind::Factory => &FACTORY,
            BuildingKind::GunTurret => &GUN_TURRET,
            BuildingKind::Wall => &WALL,
            BuildingKind::Helipad => &HELIPAD,
            BuildingKind::FlakCannon => &FLAK_CANNON,
            BuildingKind::TechCenter => &TECH_CENTER,
            BuildingKind::RepairBay => &REPAIR_BAY,
            BuildingKind::RocketBunker => &ROCKET_BUNKER,
        }
    }
}

const RIFLE: WeaponSpec = WeaponSpec {
    range: 8.0,
    damage: 9.0,
    reload_secs: 0.8,
    projectile_speed: 60.0,
    splash_radius: 0.0,
    style: ProjectileStyle::Bullet,
    versus_infantry: 1.0,
    versus_vehicle: 0.2,
    versus_structure: 0.25,
    versus_aircraft: 0.25,
    report: Sound::RifleShot,
};

const SCOUT_MACHINE_GUN: WeaponSpec = WeaponSpec {
    range: 10.0,
    damage: 7.0,
    reload_secs: 0.35,
    projectile_speed: 70.0,
    splash_radius: 0.0,
    style: ProjectileStyle::Bullet,
    versus_infantry: 1.0,
    versus_vehicle: 0.45,
    versus_structure: 0.3,
    versus_aircraft: 0.55,
    report: Sound::MachineGunShot,
};

const TANK_CANNON: WeaponSpec = WeaponSpec {
    range: 11.0,
    damage: 45.0,
    reload_secs: 1.6,
    projectile_speed: 40.0,
    splash_radius: 0.0,
    style: ProjectileStyle::Shell,
    versus_infantry: 0.6,
    versus_vehicle: 1.0,
    versus_structure: 0.8,
    versus_aircraft: 0.0,
    report: Sound::CannonShot,
};

const HOWITZER: WeaponSpec = WeaponSpec {
    range: 20.0,
    damage: 80.0,
    reload_secs: 4.0,
    projectile_speed: 18.0,
    splash_radius: 2.2,
    style: ProjectileStyle::ArtilleryShell,
    versus_infantry: 0.5,
    versus_vehicle: 0.6,
    versus_structure: 1.2,
    versus_aircraft: 0.0,
    report: Sound::HowitzerShot,
};

const TURRET_CANNON: WeaponSpec = WeaponSpec {
    range: 12.0,
    damage: 32.0,
    reload_secs: 1.0,
    projectile_speed: 45.0,
    splash_radius: 0.0,
    style: ProjectileStyle::Shell,
    versus_infantry: 0.8,
    versus_vehicle: 1.0,
    versus_structure: 0.5,
    versus_aircraft: 0.6,
    report: Sound::CannonShot,
};

/// Bursting shells that only reach aircraft: the pit cannot depress its guns at the ground.
const FLAK_GUNS: WeaponSpec = WeaponSpec {
    range: 15.0,
    damage: 24.0,
    reload_secs: 0.5,
    projectile_speed: 70.0,
    splash_radius: 1.2,
    style: ProjectileStyle::Bullet,
    versus_infantry: 0.0,
    versus_vehicle: 0.0,
    versus_structure: 0.0,
    versus_aircraft: 2.0,
    report: Sound::FlakShot,
};

const FLAK_AUTOCANNON: WeaponSpec = WeaponSpec {
    range: 13.0,
    damage: 16.0,
    reload_secs: 0.45,
    projectile_speed: 70.0,
    splash_radius: 0.0,
    style: ProjectileStyle::Bullet,
    versus_infantry: 0.6,
    versus_vehicle: 0.25,
    versus_structure: 0.15,
    versus_aircraft: 1.8,
    report: Sound::FlakShot,
};

const SNIPER_RIFLE: WeaponSpec = WeaponSpec {
    range: 16.0,
    damage: 60.0,
    reload_secs: 2.5,
    projectile_speed: 120.0,
    splash_radius: 0.0,
    style: ProjectileStyle::Bullet,
    versus_infantry: 1.5,
    versus_vehicle: 0.05,
    versus_structure: 0.05,
    versus_aircraft: 0.0,
    report: Sound::SniperShot,
};

const FLAMETHROWER: WeaponSpec = WeaponSpec {
    range: 6.0,
    damage: 14.0,
    reload_secs: 0.25,
    projectile_speed: 22.0,
    splash_radius: 1.4,
    style: ProjectileStyle::Flame,
    versus_infantry: 1.6,
    versus_vehicle: 0.12,
    versus_structure: 0.9,
    versus_aircraft: 0.0,
    report: Sound::FlameBurst,
};

const ROCKET_SALVO: WeaponSpec = WeaponSpec {
    range: 18.0,
    damage: 55.0,
    reload_secs: 3.5,
    projectile_speed: 24.0,
    splash_radius: 2.0,
    style: ProjectileStyle::Missile,
    versus_infantry: 0.8,
    versus_vehicle: 0.9,
    versus_structure: 1.1,
    versus_aircraft: 0.0,
    report: Sound::MissileLaunch,
};

const BUNKER_ROCKETS: WeaponSpec = WeaponSpec {
    range: 13.0,
    damage: 40.0,
    reload_secs: 1.6,
    projectile_speed: 30.0,
    splash_radius: 0.0,
    style: ProjectileStyle::Missile,
    versus_infantry: 0.5,
    versus_vehicle: 1.1,
    versus_structure: 0.5,
    versus_aircraft: 0.8,
    report: Sound::MissileLaunch,
};

const RIFLEMAN: UnitSpec = UnitSpec {
    name: "Rifleman",
    role: "Cheap infantry. Strong against soldiers, weak against armor.",
    model_id: "rifleman",
    cost: 100,
    build_secs: 5.0,
    max_health: 50.0,
    speed: 2.4,
    turn_rate: 10.0,
    radius: 0.4,
    armor: ArmorClass::Infantry,
    weapon: Some(RIFLE),
    has_turret: false,
    producer: BuildingKind::Barracks,
    requires: None,
    faction: None,
    heal_per_sec: 0.0,
    sight: 14.0,
};

const SCOUT: UnitSpec = UnitSpec {
    name: "Scout Buggy",
    role: "Fast wheeled raider with a roof machine gun. Hunts infantry.",
    model_id: "scout",
    cost: 300,
    build_secs: 8.0,
    max_health: 150.0,
    speed: 7.0,
    turn_rate: 5.0,
    radius: 1.0,
    armor: ArmorClass::Vehicle,
    weapon: Some(SCOUT_MACHINE_GUN),
    has_turret: true,
    producer: BuildingKind::Factory,
    requires: None,
    faction: None,
    heal_per_sec: 0.0,
    sight: 22.0,
};

const TANK: UnitSpec = UnitSpec {
    name: "Battle Tank",
    role: "Main battle tank. Heavy cannon, thick armor.",
    model_id: "tank",
    cost: 700,
    build_secs: 14.0,
    max_health: 420.0,
    speed: 3.6,
    turn_rate: 2.5,
    radius: 1.3,
    armor: ArmorClass::Vehicle,
    weapon: Some(TANK_CANNON),
    has_turret: true,
    producer: BuildingKind::Factory,
    requires: None,
    faction: None,
    heal_per_sec: 0.0,
    sight: 16.0,
};

const ARTILLERY: UnitSpec = UnitSpec {
    name: "Howitzer",
    role: "Long-range artillery. Splash damage, devastating against bases.",
    model_id: "artillery",
    cost: 900,
    build_secs: 18.0,
    max_health: 200.0,
    speed: 2.8,
    turn_rate: 2.0,
    radius: 1.3,
    armor: ArmorClass::Vehicle,
    weapon: Some(HOWITZER),
    has_turret: true,
    producer: BuildingKind::Factory,
    requires: Some(BuildingKind::TechCenter),
    faction: None,
    heal_per_sec: 0.0,
    sight: 14.0,
};

const HARVESTER: UnitSpec = UnitSpec {
    name: "Harvester",
    role: "Mines crystals and delivers them to a refinery. Unarmed.",
    model_id: "harvester",
    cost: 1000,
    build_secs: 16.0,
    max_health: 600.0,
    speed: 3.0,
    turn_rate: 2.0,
    radius: 1.5,
    armor: ArmorClass::Vehicle,
    weapon: None,
    has_turret: false,
    producer: BuildingKind::Factory,
    requires: None,
    faction: None,
    heal_per_sec: 0.0,
    sight: 10.0,
};

const MISSILE_LAUNCHER: WeaponSpec = WeaponSpec {
    range: 9.5,
    damage: 42.0,
    reload_secs: 2.2,
    projectile_speed: 26.0,
    splash_radius: 0.0,
    style: ProjectileStyle::Missile,
    versus_infantry: 0.3,
    versus_vehicle: 1.25,
    versus_structure: 0.7,
    versus_aircraft: 1.1,
    report: Sound::MissileLaunch,
};

const TWIN_CANNON: WeaponSpec = WeaponSpec {
    range: 11.5,
    damage: 62.0,
    reload_secs: 1.7,
    projectile_speed: 38.0,
    splash_radius: 0.0,
    style: ProjectileStyle::Shell,
    versus_infantry: 0.6,
    versus_vehicle: 1.0,
    versus_structure: 1.0,
    versus_aircraft: 0.0,
    report: Sound::HeavyCannonShot,
};

const ROCKETEER: UnitSpec = UnitSpec {
    name: "Rocketeer",
    role: "Anti-armour infantry with a guided missile launcher. Weak against soldiers.",
    model_id: "rocketeer",
    cost: 250,
    build_secs: 7.0,
    max_health: 60.0,
    speed: 2.2,
    turn_rate: 10.0,
    radius: 0.4,
    armor: ArmorClass::Infantry,
    weapon: Some(MISSILE_LAUNCHER),
    has_turret: false,
    producer: BuildingKind::Barracks,
    requires: None,
    faction: None,
    heal_per_sec: 0.0,
    sight: 14.0,
};

const MEDIC: UnitSpec = UnitSpec {
    name: "Field Medic",
    role: "Unarmed. Treats wounded infantry nearby and follows the squad.",
    model_id: "medic",
    cost: 200,
    build_secs: 6.0,
    max_health: 60.0,
    speed: 2.4,
    turn_rate: 10.0,
    radius: 0.4,
    armor: ArmorClass::Infantry,
    weapon: None,
    has_turret: false,
    producer: BuildingKind::Barracks,
    requires: None,
    faction: None,
    heal_per_sec: 8.0,
    sight: 12.0,
};

const ENGINEER: UnitSpec = UnitSpec {
    name: "Engineer",
    role: "Unarmed. Right-click an enemy structure to capture it, or a damaged own one to repair it fully.",
    model_id: "engineer",
    cost: 500,
    build_secs: 10.0,
    max_health: 50.0,
    speed: 2.6,
    turn_rate: 10.0,
    radius: 0.4,
    armor: ArmorClass::Infantry,
    weapon: None,
    has_turret: false,
    producer: BuildingKind::Barracks,
    requires: None,
    faction: None,
    heal_per_sec: 0.0,
    sight: 12.0,
};

const ROCKET_PODS: WeaponSpec = WeaponSpec {
    range: 9.0,
    damage: 30.0,
    reload_secs: 1.3,
    projectile_speed: 32.0,
    splash_radius: 0.0,
    style: ProjectileStyle::Missile,
    versus_infantry: 0.9,
    versus_vehicle: 1.1,
    versus_structure: 0.7,
    versus_aircraft: 0.4,
    report: Sound::MissileLaunch,
};

const GUNSHIP: UnitSpec = UnitSpec {
    name: "Gunship",
    role: "Attack helicopter with rocket pods. Flies over anything; tanks and howitzers cannot hit it.",
    model_id: "gunship",
    cost: 1100,
    build_secs: 18.0,
    max_health: 320.0,
    speed: 6.5,
    turn_rate: 3.0,
    radius: 1.6,
    armor: ArmorClass::Aircraft,
    weapon: Some(ROCKET_PODS),
    has_turret: false,
    producer: BuildingKind::Helipad,
    requires: Some(BuildingKind::TechCenter),
    faction: None,
    heal_per_sec: 0.0,
    sight: 20.0,
};

const BULWARK: UnitSpec = UnitSpec {
    name: "Bulwark",
    role: "Heavy assault tank with twin cannons. Slow but very hard to stop.",
    model_id: "bulwark",
    cost: 1200,
    build_secs: 24.0,
    max_health: 1100.0,
    speed: 2.2,
    turn_rate: 1.6,
    radius: 1.6,
    armor: ArmorClass::Vehicle,
    weapon: Some(TWIN_CANNON),
    has_turret: true,
    producer: BuildingKind::Factory,
    requires: Some(BuildingKind::TechCenter),
    faction: Some(Faction::Coalition),
    heal_per_sec: 0.0,
    sight: 16.0,
};

const FLAK_TRACK: UnitSpec = UnitSpec {
    name: "Flak Track",
    role: "Mobile anti-aircraft gun. Shreds gunships, weak against armour.",
    model_id: "flak_track",
    cost: 700,
    build_secs: 11.0,
    max_health: 260.0,
    speed: 4.2,
    turn_rate: 3.5,
    radius: 1.5,
    armor: ArmorClass::Vehicle,
    weapon: Some(FLAK_AUTOCANNON),
    has_turret: true,
    producer: BuildingKind::Factory,
    requires: None,
    faction: None,
    heal_per_sec: 0.0,
    sight: 16.0,
};

const SNIPER: UnitSpec = UnitSpec {
    name: "Sniper",
    role: "Marksman who drops infantry from far away. Useless against armour.",
    model_id: "sniper",
    cost: 500,
    build_secs: 9.0,
    max_health: 60.0,
    speed: 2.3,
    turn_rate: 6.0,
    radius: 0.4,
    armor: ArmorClass::Infantry,
    weapon: Some(SNIPER_RIFLE),
    has_turret: false,
    producer: BuildingKind::Barracks,
    requires: None,
    faction: Some(Faction::Coalition),
    heal_per_sec: 0.0,
    sight: 20.0,
};

const FLAME_TROOPER: UnitSpec = UnitSpec {
    name: "Flame Trooper",
    role: "Burns out infantry and buildings at close range. Weak against armour.",
    model_id: "flame_trooper",
    cost: 300,
    build_secs: 7.0,
    max_health: 100.0,
    speed: 2.6,
    turn_rate: 8.0,
    radius: 0.4,
    armor: ArmorClass::Infantry,
    weapon: Some(FLAMETHROWER),
    has_turret: false,
    producer: BuildingKind::Barracks,
    requires: None,
    faction: Some(Faction::Syndicate),
    heal_per_sec: 0.0,
    sight: 9.0,
};

const MISSILE_TRUCK: UnitSpec = UnitSpec {
    name: "Missile Truck",
    role: "Rocket artillery on wheels. Outranges defenses, fragile up close.",
    model_id: "missile_truck",
    cost: 1000,
    build_secs: 18.0,
    max_health: 220.0,
    speed: 4.0,
    turn_rate: 2.5,
    radius: 1.4,
    armor: ArmorClass::Vehicle,
    weapon: Some(ROCKET_SALVO),
    has_turret: true,
    producer: BuildingKind::Factory,
    requires: Some(BuildingKind::TechCenter),
    faction: Some(Faction::Syndicate),
    heal_per_sec: 0.0,
    sight: 14.0,
};

const COMMAND_CENTER: BuildingSpec = BuildingSpec {
    name: "Command Center",
    role: "Heart of the base. Lose every structure and the war is lost.",
    model_id: "command_center",
    footprint: IVec2::new(3, 3),
    cost: 0,
    build_secs: 0.0,
    max_health: 1600.0,
    power: 20,
    prerequisites: &[],
    weapon: None,
    sight: 18.0,
    faction: None,
};

const POWER_PLANT: BuildingSpec = BuildingSpec {
    name: "Power Plant",
    role: "Generates 100 power. Low power halves production speed.",
    model_id: "power_plant",
    footprint: IVec2::new(2, 2),
    cost: 300,
    build_secs: 9.0,
    max_health: 500.0,
    power: 100,
    prerequisites: &[BuildingKind::CommandCenter],
    weapon: None,
    sight: 10.0,
    faction: None,
};

const REFINERY: BuildingSpec = BuildingSpec {
    name: "Refinery",
    role: "Turns crystals into credits. Comes with a free harvester.",
    model_id: "refinery",
    footprint: IVec2::new(3, 3),
    cost: 1500,
    build_secs: 18.0,
    max_health: 900.0,
    power: -40,
    prerequisites: &[BuildingKind::PowerPlant],
    weapon: None,
    sight: 12.0,
    faction: None,
};

const BARRACKS: BuildingSpec = BuildingSpec {
    name: "Barracks",
    role: "Trains infantry.",
    model_id: "barracks",
    footprint: IVec2::new(2, 2),
    cost: 400,
    build_secs: 11.0,
    max_health: 600.0,
    power: -20,
    prerequisites: &[BuildingKind::PowerPlant],
    weapon: None,
    sight: 10.0,
    faction: None,
};

const FACTORY: BuildingSpec = BuildingSpec {
    name: "War Factory",
    role: "Builds vehicles, including harvesters.",
    model_id: "factory",
    footprint: IVec2::new(3, 3),
    cost: 1800,
    build_secs: 22.0,
    max_health: 1000.0,
    power: -50,
    prerequisites: &[BuildingKind::Refinery],
    weapon: None,
    sight: 12.0,
    faction: None,
};

const GUN_TURRET: BuildingSpec = BuildingSpec {
    name: "Gun Turret",
    role: "Static defense cannon. Good against vehicles.",
    model_id: "gun_turret",
    footprint: IVec2::new(1, 1),
    cost: 600,
    build_secs: 10.0,
    max_health: 500.0,
    power: -25,
    prerequisites: &[BuildingKind::Barracks],
    weapon: Some(TURRET_CANNON),
    sight: 16.0,
    faction: None,
};

const WALL: BuildingSpec = BuildingSpec {
    name: "Wall",
    role: "Concrete barrier. Drag to lay a line; each segment is paid as it is placed.",
    model_id: "wall",
    footprint: IVec2::new(1, 1),
    cost: 50,
    build_secs: 0.0,
    max_health: 600.0,
    power: 0,
    prerequisites: &[BuildingKind::Barracks],
    weapon: None,
    sight: 4.0,
    faction: None,
};

const HELIPAD: BuildingSpec = BuildingSpec {
    name: "Helipad",
    role: "Builds gunships and repairs aircraft hovering over the pad.",
    model_id: "helipad",
    footprint: IVec2::new(2, 2),
    cost: 1000,
    build_secs: 14.0,
    max_health: 700.0,
    power: -30,
    prerequisites: &[BuildingKind::Factory],
    weapon: None,
    sight: 12.0,
    faction: None,
};

const FLAK_CANNON: BuildingSpec = BuildingSpec {
    name: "Flak Cannon",
    role: "Anti-aircraft gun pit. Cannot shoot at ground targets.",
    model_id: "flak_cannon",
    footprint: IVec2::new(1, 1),
    cost: 700,
    build_secs: 10.0,
    max_health: 450.0,
    power: -25,
    prerequisites: &[BuildingKind::Barracks],
    weapon: Some(FLAK_GUNS),
    sight: 18.0,
    faction: None,
};

const TECH_CENTER: BuildingSpec = BuildingSpec {
    name: "Tech Center",
    role: "Research labs. Unlocks heavy units: Bulwark or Missile Truck, Howitzer and Gunship.",
    model_id: "tech_center",
    footprint: IVec2::new(2, 2),
    cost: 1500,
    build_secs: 20.0,
    max_health: 800.0,
    power: -60,
    prerequisites: &[BuildingKind::Factory],
    weapon: None,
    sight: 12.0,
    faction: None,
};

const REPAIR_BAY: BuildingSpec = BuildingSpec {
    name: "Repair Bay",
    role: "Repairs vehicles that wait next to it.",
    model_id: "repair_bay",
    footprint: IVec2::new(2, 2),
    cost: 800,
    build_secs: 12.0,
    max_health: 700.0,
    power: -20,
    prerequisites: &[BuildingKind::Factory],
    weapon: None,
    sight: 10.0,
    faction: None,
};

const ROCKET_BUNKER: BuildingSpec = BuildingSpec {
    name: "Rocket Bunker",
    role: "Concrete dome with a rocket launcher. Hits vehicles and aircraft alike.",
    model_id: "rocket_bunker",
    footprint: IVec2::new(1, 1),
    cost: 800,
    build_secs: 12.0,
    max_health: 650.0,
    power: -30,
    prerequisites: &[BuildingKind::Barracks],
    weapon: Some(BUNKER_ROCKETS),
    sight: 15.0,
    faction: Some(Faction::Syndicate),
};

pub const STARTING_CREDITS: i32 = 5000;
pub const HARVESTER_CAPACITY: u32 = 500;
pub const CRYSTAL_PER_CELL: u32 = 600;

#[cfg(test)]
mod tests {
    use super::*;

    fn reachable_from_command_center() -> Vec<BuildingKind> {
        let mut owned = vec![BuildingKind::CommandCenter];
        loop {
            let next = BuildingKind::CONSTRUCTIBLE.into_iter().find(|kind| {
                !owned.contains(kind) && kind.spec().prerequisites.iter().all(|p| owned.contains(p))
            });
            match next {
                Some(kind) => owned.push(kind),
                None => return owned,
            }
        }
    }

    #[test]
    fn the_whole_tech_tree_is_reachable() {
        let owned = reachable_from_command_center();
        for kind in BuildingKind::CONSTRUCTIBLE {
            assert!(owned.contains(&kind), "{kind:?} can never be built");
        }
        for kind in UnitKind::INFANTRY.iter().chain(UnitKind::VEHICLES.iter()) {
            assert!(
                owned.contains(&kind.spec().producer),
                "{kind:?} has no buildable producer"
            );
            if let Some(required) = kind.spec().requires {
                assert!(
                    owned.contains(&required),
                    "{kind:?} needs an unbuildable {required:?}"
                );
            }
        }
    }

    #[test]
    fn weapons_have_a_clear_role() {
        let rifle = UnitKind::Rifleman.spec().weapon.unwrap();
        let cannon = UnitKind::Tank.spec().weapon.unwrap();
        let howitzer = UnitKind::Artillery.spec().weapon.unwrap();
        assert!(
            rifle.multiplier_against(ArmorClass::Infantry)
                > rifle.multiplier_against(ArmorClass::Vehicle)
        );
        assert!(
            cannon.multiplier_against(ArmorClass::Vehicle)
                > rifle.multiplier_against(ArmorClass::Vehicle)
        );
        assert!(howitzer.range > cannon.range && howitzer.splash_radius > 0.0);
    }

    #[test]
    fn a_base_can_power_its_own_production() {
        let supply: i32 = [BuildingKind::CommandCenter, BuildingKind::PowerPlant]
            .iter()
            .map(|k| k.spec().power)
            .sum();
        let demand: i32 = [BuildingKind::Refinery, BuildingKind::Barracks]
            .iter()
            .map(|k| -k.spec().power)
            .sum();
        assert!(supply >= demand);
        assert!(
            STARTING_CREDITS
                >= BuildingKind::PowerPlant.spec().cost + BuildingKind::Refinery.spec().cost
        );
    }
}
