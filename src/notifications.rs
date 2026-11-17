use bevy::prelude::*;

use crate::catalog::Team;
use crate::components::{Building, Health};
use crate::economy::Economies;
use crate::sound::{Sound, SoundCue};

const MESSAGE_LIFETIME_SECS: f32 = 4.0;
const MAX_VISIBLE_MESSAGES: usize = 4;
const ATTACK_WARNING_COOLDOWN_SECS: f32 = 12.0;

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
    previous_health: f32,
    low_power: bool,
    warning_cooldown: f32,
}

pub fn watch_base(
    mut cues: MessageWriter<SoundCue>,
    time: Res<Time>,
    economies: Res<Economies>,
    mut notifications: ResMut<Notifications>,
    mut watch: Local<BaseWatch>,
    buildings: Query<(&Health, &Team), With<Building>>,
) {
    let dt = time.delta_secs();
    notifications.age(dt);
    watch.warning_cooldown -= dt;

    let base_health: f32 = buildings
        .iter()
        .filter(|(_, team)| **team == Team::Player)
        .map(|(health, _)| health.current)
        .sum();
    if base_health < watch.previous_health - 0.5 && watch.warning_cooldown <= 0.0 {
        notifications.push("Our base is under attack!");
        cues.write(SoundCue::interface(Sound::BaseAlert));
        watch.warning_cooldown = ATTACK_WARNING_COOLDOWN_SECS;
    }
    watch.previous_health = base_health;

    let low_power = economies.of(Team::Player).has_low_power();
    if low_power && !watch.low_power {
        notifications.push("Low power - production slowed");
    }
    watch.low_power = low_power;
}
