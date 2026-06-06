mod auth;
mod core;
mod game;
mod network;
mod states;

use bevy::prelude::*;
use bevy_egui::EguiPlugin;
use states::AppState;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "MMO Client".into(),
                resolution: bevy::window::WindowResolution::new(1280, 720),
                present_mode: bevy::window::PresentMode::AutoNoVsync,
                ..default()
            }),
            ..default()
        }))
        .add_plugins(EguiPlugin::default())
        .init_state::<AppState>()

        .add_plugins((
            core::CorePlugin,
            auth::AuthPlugin,
            network::NetworkPlugin,
            game::GamePlugin,
        ))
        .run();
}