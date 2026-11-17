#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

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

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(game_window()),
            ..default()
        }))
        .add_plugins(GamePlugin)
        .run();
}
