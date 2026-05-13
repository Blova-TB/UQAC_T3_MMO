mod player;
use player::PlayerPlugin;

mod debug_tool;
use debug_tool::DebugToolPlugin;

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
            PlayerPlugin,
            DebugToolPlugin,
        ))
        .insert_resource(Gravity(Vec2::ZERO))
        .add_systems(Startup, server_startup)
        .run();
}

fn server_startup() {
    println!("Server started!");
}