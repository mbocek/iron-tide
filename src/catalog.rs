use bevy::prelude::*;
use serde::{Deserialize, Serialize};

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

    pub fn is_hostile_to(self, other: Team) -> bool {
        (self == Team::Player) != (other == Team::Player)
    }

    pub fn name(self) -> &'static str {
        match self {
            Team::Player => "Coalition",
            Team::Crimson => "Crimson Syndicate",
            Team::Amber => "Amber Syndicate",
            Team::Violet => "Violet Syndicate",
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

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ArmorClass {
    Infantry,
    Vehicle,
    Structure,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProjectileStyle {
    Bullet,
    Shell,
    ArtilleryShell,
    Missile,
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
}

impl WeaponSpec {
    pub fn multiplier_against(&self, armor: ArmorClass) -> f32 {
        match armor {
            ArmorClass::Infantry => self.versus_infantry,
            ArmorClass::Vehicle => self.versus_vehicle,
            ArmorClass::Structure => self.versus_structure,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub enum UnitKind {
    Rifleman,
    Rocketeer,
    Medic,
    Scout,
    Tank,
    Bulwark,
    Artillery,
    Harvester,
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
    /// Health restored per second to wounded friendly infantry nearby; zero for everyone but medics.
    pub heal_per_sec: f32,
}

impl UnitKind {
    pub const INFANTRY: [UnitKind; 3] = [UnitKind::Rifleman, UnitKind::Rocketeer, UnitKind::Medic];
    pub const VEHICLES: [UnitKind; 5] = [
        UnitKind::Scout,
        UnitKind::Tank,
        UnitKind::Bulwark,
        UnitKind::Artillery,
        UnitKind::Harvester,
    ];
    pub const ALL: [UnitKind; 8] = [
        UnitKind::Rifleman,
        UnitKind::Rocketeer,
        UnitKind::Medic,
        UnitKind::Scout,
        UnitKind::Tank,
        UnitKind::Bulwark,
        UnitKind::Artillery,
        UnitKind::Harvester,
    ];

    pub fn spec(self) -> &'static UnitSpec {
        match self {
            UnitKind::Rifleman => &RIFLEMAN,
            UnitKind::Scout => &SCOUT,
            UnitKind::Tank => &TANK,
            UnitKind::Artillery => &ARTILLERY,
            UnitKind::Harvester => &HARVESTER,
            UnitKind::Rocketeer => &ROCKETEER,
            UnitKind::Medic => &MEDIC,
            UnitKind::Bulwark => &BULWARK,
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
}

impl BuildingKind {
    pub const CONSTRUCTIBLE: [BuildingKind; 5] = [
        BuildingKind::PowerPlant,
        BuildingKind::Refinery,
        BuildingKind::Barracks,
        BuildingKind::Factory,
        BuildingKind::GunTurret,
    ];

    pub fn spec(self) -> &'static BuildingSpec {
        match self {
            BuildingKind::CommandCenter => &COMMAND_CENTER,
            BuildingKind::PowerPlant => &POWER_PLANT,
            BuildingKind::Refinery => &REFINERY,
            BuildingKind::Barracks => &BARRACKS,
            BuildingKind::Factory => &FACTORY,
            BuildingKind::GunTurret => &GUN_TURRET,
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
    versus_vehicle: 0.3,
    versus_structure: 0.25,
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
};

const TANK_CANNON: WeaponSpec = WeaponSpec {
    range: 11.0,
    damage: 45.0,
    reload_secs: 1.6,
    projectile_speed: 40.0,
    splash_radius: 0.0,
    style: ProjectileStyle::Shell,
    versus_infantry: 0.4,
    versus_vehicle: 1.0,
    versus_structure: 0.8,
};

const HOWITZER: WeaponSpec = WeaponSpec {
    range: 20.0,
    damage: 80.0,
    reload_secs: 4.0,
    projectile_speed: 18.0,
    splash_radius: 3.0,
    style: ProjectileStyle::ArtilleryShell,
    versus_infantry: 1.0,
    versus_vehicle: 0.6,
    versus_structure: 1.2,
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
    heal_per_sec: 0.0,
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
    heal_per_sec: 0.0,
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
    heal_per_sec: 0.0,
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
    heal_per_sec: 0.0,
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
    heal_per_sec: 0.0,
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
};

const TWIN_CANNON: WeaponSpec = WeaponSpec {
    range: 11.5,
    damage: 62.0,
    reload_secs: 2.2,
    projectile_speed: 38.0,
    splash_radius: 0.0,
    style: ProjectileStyle::Shell,
    versus_infantry: 0.5,
    versus_vehicle: 1.0,
    versus_structure: 1.0,
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
    heal_per_sec: 0.0,
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
    heal_per_sec: 8.0,
};

const BULWARK: UnitSpec = UnitSpec {
    name: "Bulwark",
    role: "Heavy assault tank with twin cannons. Slow but very hard to stop.",
    model_id: "bulwark",
    cost: 1500,
    build_secs: 24.0,
    max_health: 950.0,
    speed: 2.2,
    turn_rate: 1.6,
    radius: 1.6,
    armor: ArmorClass::Vehicle,
    weapon: Some(TWIN_CANNON),
    has_turret: true,
    producer: BuildingKind::Factory,
    heal_per_sec: 0.0,
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
