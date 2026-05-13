use bevy::prelude::*;
use avian2d::prelude::*;

// --- Plugin ---

pub struct PlayerPlugin;

impl Plugin for PlayerPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_initial_player);
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
            transform: Transform::from_translation(position.extend(0.0)),
            global_transform: GlobalTransform::default(),
        }
    }
}

// --- Systèmes ---

pub fn spawn_initial_player(mut commands: Commands) {
    let start_position = Vec2::ZERO;

    commands.spawn(PlayerBundle::new(
        start_position,
        100,    // health
        300.0,  // speed
        15.0,   // radius
    ));

    println!("Player spawned at {:?}", start_position);
}