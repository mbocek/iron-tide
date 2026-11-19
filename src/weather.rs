use bevy::prelude::*;
use rand::rngs::StdRng;
use rand::{RngExt, SeedableRng};

use crate::levels::{ActiveLevel, Weather};
use crate::season::Season;
use crate::terrain_material::{TerrainMaterial, TerrainMaterialHandle};

const CLEAR_SUNLIGHT: f32 = 7_000.0;
const RAIN_SUNLIGHT: f32 = 3_200.0;
const CLEAR_AMBIENT: f32 = 280.0;
const RAIN_AMBIENT: f32 = 190.0;
/// How long one kind of sky takes to turn into the next.
const TRANSITION_SECS: f32 = 30.0;
const CHANCE_CLOUDS_BRING_RAIN: f64 = 0.6;
/// Far longer than any match, so the forecast never runs out.
const FORECAST_SECS: f32 = 6.0 * 3600.0;
/// The ground soaks within half a minute of rain and takes a couple of minutes to dry.
const SOAKING_PER_SEC: f32 = 1.0 / 25.0;
const DRYING_PER_SEC: f32 = 1.0 / 120.0;
/// Keeps the weather's random sequence apart from the map's, which also starts from the map seed.
const FORECAST_SALT: u64 = 0x0057_4541_5448_4552;
/// Smaller changes are not worth uploading the ground material again.
const TERRAIN_UPDATE_STEP: f32 = 0.01;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Sky {
    Clear,
    Overcast,
    Rain,
}

impl Sky {
    fn cloud_cover(self) -> f32 {
        match self {
            Sky::Clear => 0.0,
            Sky::Overcast => 0.7,
            Sky::Rain => 1.0,
        }
    }

    fn rainfall(self) -> f32 {
        if self == Sky::Rain { 1.0 } else { 0.0 }
    }

    fn lasts_secs(self, rng: &mut StdRng) -> f32 {
        match self {
            Sky::Clear => rng.random_range(150.0..330.0),
            Sky::Overcast => rng.random_range(60.0..150.0),
            Sky::Rain => rng.random_range(70.0..200.0),
        }
    }

    fn turns_into(self, rng: &mut StdRng) -> Sky {
        match self {
            Sky::Clear | Sky::Rain => Sky::Overcast,
            Sky::Overcast if rng.random_bool(CHANCE_CLOUDS_BRING_RAIN) => Sky::Rain,
            Sky::Overcast => Sky::Clear,
        }
    }
}

/// The weather of a match: the same forecast for every player of a map, as it comes from the map seed.
/// It only changes how the battlefield looks.
#[derive(Resource)]
pub struct WeatherCycle {
    /// When each spell of weather starts, in seconds of match time.
    spells: Vec<(f32, Sky)>,
    started_at: f32,
    pub now: SkyNow,
}

#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct SkyNow {
    pub cloud_cover: f32,
    pub rainfall: f32,
    pub wet_ground: f32,
}

impl WeatherCycle {
    fn forecast(map_seed: u64, first: Sky, started_at: f32) -> Self {
        let mut rng = StdRng::seed_from_u64(map_seed ^ FORECAST_SALT);
        let mut spells = vec![(0.0, first)];
        let mut sky = first;
        let mut starts = sky.lasts_secs(&mut rng);
        while starts < FORECAST_SECS {
            sky = sky.turns_into(&mut rng);
            spells.push((starts, sky));
            starts += sky.lasts_secs(&mut rng);
        }
        Self {
            spells,
            started_at,
            now: SkyNow {
                cloud_cover: first.cloud_cover(),
                rainfall: first.rainfall(),
                wet_ground: first.rainfall(),
            },
        }
    }

    /// Cloud cover and rainfall at `secs` into the match, blending into the next spell as it approaches.
    fn sky_at(&self, secs: f32) -> (f32, f32) {
        let current = self
            .spells
            .partition_point(|&(start, _)| start <= secs)
            .max(1)
            - 1;
        let (_, sky) = self.spells[current];
        let Some(&(next_starts, next)) = self.spells.get(current + 1) else {
            return (sky.cloud_cover(), sky.rainfall());
        };
        let turning = ((secs - (next_starts - TRANSITION_SECS)) / TRANSITION_SECS).clamp(0.0, 1.0);
        let blend = turning * turning * (3.0 - 2.0 * turning);
        (
            sky.cloud_cover().lerp(next.cloud_cover(), blend),
            sky.rainfall().lerp(next.rainfall(), blend),
        )
    }
}

pub fn begin_weather(mut commands: Commands, level: Res<ActiveLevel>, time: Res<Time<Virtual>>) {
    let first = match level.0.weather {
        Weather::Rain => Sky::Rain,
        Weather::Clear => Sky::Clear,
    };
    commands.insert_resource(WeatherCycle::forecast(
        level.0.map_seed,
        first,
        time.elapsed_secs(),
    ));
}

/// Moves the sky along its forecast: clouds dim the sun and the sky light, rain soaks the ground.
pub fn follow_the_weather(
    time: Res<Time<Virtual>>,
    level: Option<Res<ActiveLevel>>,
    cycle: Option<ResMut<WeatherCycle>>,
    terrain: Option<Res<TerrainMaterialHandle>>,
    mut terrain_materials: ResMut<Assets<TerrainMaterial>>,
    mut suns: Query<&mut DirectionalLight>,
    mut ambient: ResMut<GlobalAmbientLight>,
) {
    let (Some(level), Some(mut cycle)) = (level, cycle) else {
        return;
    };
    let (cloud_cover, rainfall) = cycle.sky_at(time.elapsed_secs() - cycle.started_at);
    let rate = if rainfall > cycle.now.wet_ground {
        SOAKING_PER_SEC
    } else {
        DRYING_PER_SEC
    };
    let wet_ground = cycle.now.wet_ground
        + (rainfall - cycle.now.wet_ground)
            .clamp(-rate * time.delta_secs(), rate * time.delta_secs());
    cycle.now = SkyNow {
        cloud_cover,
        rainfall,
        wet_ground,
    };

    let season = level.0.season;
    let illuminance = CLEAR_SUNLIGHT.lerp(RAIN_SUNLIGHT, cloud_cover);
    for mut sun in &mut suns {
        if sun.illuminance != illuminance || sun.color != season.sunlight_color() {
            sun.illuminance = illuminance;
            sun.color = season.sunlight_color();
        }
    }
    let brightness = CLEAR_AMBIENT.lerp(RAIN_AMBIENT, cloud_cover) * season.ambient_factor();
    if ambient.brightness != brightness {
        ambient.brightness = brightness;
    }

    let soaked = if season == Season::Winter {
        0.0
    } else {
        wet_ground
    };
    let Some(material) = terrain
        .as_ref()
        .and_then(|terrain| terrain_materials.get(&terrain.0))
    else {
        return;
    };
    let shown = material.extension.settings;
    let stale = (shown.wet_ground - soaked).abs() > TERRAIN_UPDATE_STEP
        || (shown.cloud_cover - cloud_cover).abs() > TERRAIN_UPDATE_STEP;
    if !stale {
        return;
    }
    if let Some(mut material) = terrain
        .as_ref()
        .and_then(|terrain| terrain_materials.get_mut(&terrain.0))
    {
        material.extension.settings.wet_ground = soaked;
        material.extension.settings.cloud_cover = cloud_cover;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_player_of_a_map_gets_the_same_forecast() {
        let one = WeatherCycle::forecast(7, Sky::Clear, 0.0);
        let other = WeatherCycle::forecast(7, Sky::Clear, 0.0);
        assert_eq!(one.spells, other.spells);
    }

    #[test]
    fn the_sky_changes_during_a_match_and_starts_as_the_level_says() {
        let cycle = WeatherCycle::forecast(3, Sky::Rain, 0.0);
        assert_eq!(cycle.sky_at(0.0), (1.0, 1.0));
        let skies: Vec<Sky> = cycle.spells.iter().take(6).map(|&(_, sky)| sky).collect();
        assert!(skies.contains(&Sky::Overcast));
        assert!(cycle.spells[1].0 < 600.0);
    }

    #[test]
    fn rain_fades_in_instead_of_switching_on() {
        let cycle = WeatherCycle::forecast(11, Sky::Overcast, 0.0);
        let (turn, next) = cycle.spells[1];
        let (_, halfway) = cycle.sky_at(turn - TRANSITION_SECS * 0.5);
        if next == Sky::Rain {
            assert!(halfway > 0.0 && halfway < 1.0);
        } else {
            assert_eq!(halfway, 0.0);
        }
    }
}
