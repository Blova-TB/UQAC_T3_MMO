mod broker_plugin;
mod config;
mod core;
mod events;
mod game;
mod orchestrator_plugin;
mod player;
mod states;

use avian2d::prelude::*;
use bevy::app::ScheduleRunnerPlugin;
use bevy::log::LogPlugin;
use bevy::prelude::*;
use bevy::scene::ScenePlugin;
use bevy::state::app::StatesPlugin;
use std::time::Duration;

use broker_plugin::BrokerPlugin;
use config::ServerConfig;
use core::CorePlugin;
use game::GamePlugin;
use orchestrator_plugin::OrchestratorPlugin;
use player::PlayerPlugin;
use states::ServerState;

fn main() {
    App::new()
        .add_plugins((
            MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(Duration::from_secs_f64(
                1.0 / 60.0,
            ))),
            LogPlugin::default(),
            StatesPlugin,
            AssetPlugin::default(),
            ScenePlugin,
            TransformPlugin,
            PhysicsPlugins::default(),
        ))
        .insert_resource(Gravity(Vec2::ZERO))
        .insert_resource(ServerConfig::from_env())
        .init_state::<ServerState>()
        // --- Nos Plugins Modulaires ---
        .add_plugins((
            CorePlugin,
            OrchestratorPlugin,
            BrokerPlugin,
            GamePlugin,
            PlayerPlugin,
        ))
        .run();
}