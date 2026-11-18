use bevy::prelude::*;

use crate::i18n::tr_fmt;
use crate::network::{NetSession, SessionState};
use crate::player_commands::SimulationTick;
use crate::ui::{ACCENT, SIDEBAR_WIDTH, TEXT_DIM, UiFonts, set_field};

/// A short stall is ordinary network jitter; only a longer one is worth telling the player about.
const STALL_WORTH_SHOWING_SECS: f32 = 0.5;

#[derive(Component)]
pub struct LatencyText;

#[derive(Component)]
pub struct WaitingBanner;

pub fn spawn_network_hud(commands: &mut Commands, fonts: &UiFonts) {
    commands.spawn((
        LatencyText,
        fonts.text("", 14.0, TEXT_DIM),
        TextShadow::default(),
        Pickable::IGNORE,
        Node {
            position_type: PositionType::Absolute,
            bottom: Val::Px(8.0),
            right: Val::Px(SIDEBAR_WIDTH + 12.0),
            ..default()
        },
    ));
    commands.spawn((
        WaitingBanner,
        fonts.heading("", 26.0, ACCENT),
        TextShadow::default(),
        Pickable::IGNORE,
        Node {
            position_type: PositionType::Absolute,
            top: Val::Percent(30.0),
            left: Val::Px(0.0),
            right: Val::Px(SIDEBAR_WIDTH),
            justify_content: JustifyContent::Center,
            display: Display::None,
            ..default()
        },
        TextLayout::justify(Justify::Center),
    ));
}

/// The ping in the corner, and a banner while the battle waits for the other player's orders.
pub fn refresh_network_hud(
    session: Option<Res<NetSession>>,
    tick: Res<SimulationTick>,
    real: Res<Time<Real>>,
    game_time: Res<Time<Virtual>>,
    mut last_progress: Local<(u64, f32)>,
    mut latency: Query<&mut Text, (With<LatencyText>, Without<WaitingBanner>)>,
    mut banners: Query<(&mut Text, &mut Node), With<WaitingBanner>>,
) {
    let now = real.elapsed_secs();
    if last_progress.0 != tick.0 || game_time.is_paused() {
        *last_progress = (tick.0, now);
    }
    let in_step = session
        .as_ref()
        .is_some_and(|session| session.state == SessionState::InStep);
    let ping = session
        .as_ref()
        .filter(|_| in_step)
        .and_then(|session| session.round_trip())
        .map(|round_trip| tr_fmt("Ping {} ms", &[&round_trip.as_millis()]))
        .unwrap_or_default();
    for mut text in &mut latency {
        text.set_if_neq(Text(ping.clone()));
    }
    let stalled_for = now - last_progress.1;
    let waiting = in_step && stalled_for > STALL_WORTH_SHOWING_SECS;
    for (mut text, mut node) in &mut banners {
        let display = if waiting {
            Display::Flex
        } else {
            Display::None
        };
        set_field(&mut node, |node| &mut node.display, display);
        if waiting {
            text.set_if_neq(Text(tr_fmt(
                "Waiting for the other player... {} s",
                &[&(stalled_for as u32)],
            )));
        }
    }
}
