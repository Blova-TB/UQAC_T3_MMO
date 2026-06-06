mod player;
mod debug_tool;
mod network_plugin;
mod orchestrator_plugin;
mod config;

use player::PlayerPlugin;
use debug_tool::DebugToolPlugin;
use network_plugin::NetworkServerPlugin;

use bevy::prelude::*;
use avian2d::prelude::*;
use bevy::state::app::StatesPlugin;
use bevy::app::ScheduleRunnerPlugin;
use std::time::Duration;
use bevy::scene::ScenePlugin;
use crate::orchestrator_plugin::OrchestratorPlugin;
use config::ServerConfig;

#[derive(States, Debug, Clone, Copy, Eq, PartialEq, Hash, Default)]
pub enum ServerState {
    #[default]
    WaitingAssignment,
    Active,
}

fn main() {
    App::new()
        .add_plugins((
            MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(Duration::from_secs_f64(1.0 / 60.0))),
            StatesPlugin,
            AssetPlugin::default(),
            ScenePlugin,
            TransformPlugin,
            PhysicsPlugins::default(),
        ))
        .add_plugins((
            OrchestratorPlugin,
            NetworkServerPlugin,
            PlayerPlugin,
            DebugToolPlugin,
        ))
        .insert_resource(Gravity(Vec2::ZERO))
        .insert_resource(ServerConfig::from_env())
        .init_state::<ServerState>()
        .run();
}