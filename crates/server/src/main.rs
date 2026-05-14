mod player;
mod debug_tool;
mod network;

use player::PlayerPlugin;
use debug_tool::DebugToolPlugin;
use network::NetworkServerPlugin;

use bevy::prelude::*;
use avian2d::prelude::*;
use bevy::app::ScheduleRunnerPlugin;
use std::time::Duration;
use bevy::scene::ScenePlugin;


fn main() {
    App::new()
        .add_plugins((
            MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(Duration::from_secs_f64(1.0 / 60.0))),
            AssetPlugin::default(),
            ScenePlugin,
            TransformPlugin,
            PhysicsPlugins::default(),
        ))
        .add_plugins((
            NetworkServerPlugin,
            PlayerPlugin,
            DebugToolPlugin,
        ))
        .insert_resource(Gravity(Vec2::ZERO))
        .run();
}