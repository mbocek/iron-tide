#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use bevy::ecs::schedule::{ScheduleLabel, SingleThreadedExecutor};
use bevy::prelude::*;
use iron_tide::{GamePlugin, capture};

fn game_window() -> Window {
    let window = Window {
        title: "Iron Tide".into(),
        resolution: (1600_u32, 900_u32).into(),
        ..default()
    };
    if !capture::is_capture_run() {
        return window;
    }
    Window {
        focused: false,
        window_level: bevy::window::WindowLevel::AlwaysOnBottom,
        present_mode: bevy::window::PresentMode::AutoNoVsync,
        ..window
    }
}

/// Most game systems are chained, so the multi-threaded executor had little to run in parallel and spent
/// more on waking worker threads than it saved: one thread measured about 13 % less CPU per frame.
fn run_game_schedules_on_one_thread(app: &mut App) {
    let schedules = [
        First.intern(),
        PreUpdate.intern(),
        Update.intern(),
        PostUpdate.intern(),
        Last.intern(),
    ];
    for schedule in schedules {
        app.edit_schedule(schedule, |schedule| {
            schedule.set_executor(SingleThreadedExecutor::new());
        });
    }
}

fn main() {
    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(game_window()),
        ..default()
    }))
    .add_plugins(GamePlugin);
    run_game_schedules_on_one_thread(&mut app);
    app.run();
}
