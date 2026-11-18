use bevy::prelude::*;

use crate::combat::DamageDealt;
use crate::components::{Building, Harvester};
use crate::economy::Economies;
use crate::i18n::tr;
use crate::player_commands::LocalTeam;
use crate::sound::{Sound, SoundCue};

const MESSAGE_LIFETIME_SECS: f32 = 4.0;
const MAX_VISIBLE_MESSAGES: usize = 4;
const ATTACK_WARNING_COOLDOWN_SECS: f32 = 12.0;

/// Where the latest attack on the local side was reported, for the jump-to-alert key.
#[derive(Resource, Default)]
pub struct LastAlert(pub Option<Vec3>);

/// Short status lines for the player, shown at the top of the battlefield.
#[derive(Resource, Default)]
pub struct Notifications {
    messages: Vec<(String, f32)>,
}

impl Notifications {
    pub fn push(&mut self, message: impl Into<String>) {
        let message = message.into();
        self.messages.retain(|(existing, _)| *existing != message);
        self.messages.push((message, MESSAGE_LIFETIME_SECS));
        if self.messages.len() > MAX_VISIBLE_MESSAGES {
            self.messages.remove(0);
        }
    }

    pub fn visible(&self) -> impl Iterator<Item = (&str, f32)> {
        self.messages
            .iter()
            .map(|(message, remaining)| (message.as_str(), (remaining / 0.6).min(1.0)))
    }

    fn age(&mut self, dt: f32) {
        for (_, remaining) in &mut self.messages {
            *remaining -= dt;
        }
        self.messages.retain(|(_, remaining)| *remaining > 0.0);
    }
}

#[derive(Default)]
pub struct BaseWatch {
    low_power: bool,
    warning_cooldown: f32,
    harvester_warning_cooldown: f32,
}

pub fn watch_base(
    local: Res<LocalTeam>,
    mut cues: MessageWriter<SoundCue>,
    time: Res<Time>,
    economies: Res<Economies>,
    mut notifications: ResMut<Notifications>,
    mut alert: ResMut<LastAlert>,
    mut watch: Local<BaseWatch>,
    mut hits: MessageReader<DamageDealt>,
    victims: Query<(&Transform, Has<Building>, Has<Harvester>)>,
) {
    let dt = time.delta_secs();
    notifications.age(dt);
    watch.warning_cooldown -= dt;
    watch.harvester_warning_cooldown -= dt;

    for hit in hits.read().filter(|hit| hit.team == local.0) {
        let Ok((transform, is_building, is_harvester)) = victims.get(hit.victim) else {
            continue;
        };
        let warning = if is_building && watch.warning_cooldown <= 0.0 {
            watch.warning_cooldown = ATTACK_WARNING_COOLDOWN_SECS;
            tr("Our base is under attack!")
        } else if is_harvester && watch.harvester_warning_cooldown <= 0.0 {
            watch.harvester_warning_cooldown = ATTACK_WARNING_COOLDOWN_SECS;
            tr("Our harvester is under attack!")
        } else {
            continue;
        };
        notifications.push(warning);
        cues.write(SoundCue::interface(Sound::BaseAlert));
        alert.0 = Some(transform.translation);
    }

    let low_power = economies.of(local.0).has_low_power();
    if low_power && !watch.low_power {
        notifications.push(tr("Low power - production slowed"));
    }
    watch.low_power = low_power;
}
