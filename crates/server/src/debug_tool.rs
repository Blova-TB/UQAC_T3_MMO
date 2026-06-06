use bevy::prelude::*;
use crate::player::Player;

pub struct DebugToolPlugin;

impl Plugin for DebugToolPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                nothing,
                // debug_print,
                // debug_system,
                // print_player_positions
            )
                .chain(),
        );
    }
}

fn debug_system() {
    println!("Debug: System is running!");
}

fn print_player_positions(query: Query<(Entity, &Transform), With<Player>>) {
    for (entity, transform) in query.iter() {
        let position_2d = transform.translation.truncate();

        println!(
            "Entity {:?} (Player) is at position: {:?}",
            entity, position_2d
        );
    }
}

fn debug_print(query: Query<(Entity, &Transform), With<Player>>) {
    println!("Players connected: {}",query.iter().count());
}

fn nothing() {}