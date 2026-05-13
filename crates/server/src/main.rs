mod player;
mod debug_tool;

use player::PlayerPlugin;
use debug_tool::DebugToolPlugin;

use bevy::prelude::*;
use avian2d::prelude::*;

fn main() {
    App::new()
        //.add_plugins((MinimalPlugins,TransformPlugin))    // opti
        .add_plugins(DefaultPlugins)              // fonctionnel
        .add_plugins(PhysicsPlugins::default())
        .add_plugins(PlayerPlugin)
        .add_plugins(DebugToolPlugin)
        .add_systems(Startup, server_startup)
        .run();
}

fn server_startup() {
    println!("Server started!");
}