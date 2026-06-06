use avian2d::prelude::*;
use bevy::prelude::*;
use shared::game_protocol::{INPUT_DOWN, INPUT_LEFT, INPUT_RIGHT, INPUT_UP};

// ✨ L'importation vitale qui manquait pour résoudre l'erreur E0412
use crate::game::PlayerInputState;

pub struct PlayerPlugin;

impl Plugin for PlayerPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(FixedUpdate, apply_player_inputs);
    }
}

// --- Composants ---

#[derive(Component)]
pub struct Player;

#[derive(Component)]
pub struct Health(pub u32);

#[derive(Component)]
pub struct MovementSpeed(pub f32);

// --- Bundle ---

#[derive(Bundle)]
pub struct PlayerBundle {
    player: Player,
    health: Health,
    speed: MovementSpeed,

    rigid_body: RigidBody,
    collider: Collider,
    linear_damping: LinearDamping,
    locked_axes: LockedAxes,
    velocity: LinearVelocity,

    transform: Transform,
    global_transform: GlobalTransform,
}

impl PlayerBundle {
    pub fn new(position: Vec2, health: u32, speed: f32, radius: f32) -> Self {
        Self {
            player: Player,
            health: Health(health),
            speed: MovementSpeed(speed),

            rigid_body: RigidBody::Dynamic,
            collider: Collider::circle(radius),
            linear_damping: LinearDamping(5.0),
            locked_axes: LockedAxes::ROTATION_LOCKED,
            velocity: LinearVelocity::ZERO,

            transform: Transform::from_translation(position.extend(0.0)),
            global_transform: GlobalTransform::default(),
        }
    }
}

// --- Systèmes ---

pub fn apply_player_inputs(
    mut query: Query<(&PlayerInputState, &MovementSpeed, &mut LinearVelocity)>,
) {
    for (input_state, speed, mut velocity) in query.iter_mut() {
        let mut dir = Vec2::ZERO;
        let inp = input_state.latest_input;

        if inp & INPUT_UP != 0 { dir.y += 1.0; }
        if inp & INPUT_DOWN != 0 { dir.y -= 1.0; }
        if inp & INPUT_LEFT != 0 { dir.x -= 1.0; }
        if inp & INPUT_RIGHT != 0 { dir.x += 1.0; }

        if dir != Vec2::ZERO {
            let normalized_dir = dir.normalize();
            velocity.x = normalized_dir.x * speed.0;
            velocity.y = normalized_dir.y * speed.0;
        }
    }
}