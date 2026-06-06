use crate::core::{ClientState, GameAssets, NetworkSnapshotEvent, SessionData};
use crate::states::AppState;
use bevy::prelude::*;
use shared::game_protocol::{
    CustomId, LogicalStream, PlayerInput, PlayerInputPayload, INPUT_ACTION, INPUT_DOWN, INPUT_LEFT,
    INPUT_RIGHT, INPUT_UP,
};
use shared::models::{Publish, ServerBinaryPacket};
use shared::network::{GameStream, GameStreamReliability};
use std::collections::HashMap;

pub struct GamePlugin;

impl Plugin for GamePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PlayerInputBuffer>()
            .insert_resource(Time::<Fixed>::from_hz(60.0))
            .add_systems(Startup, setup_camera)
            .add_systems(
                Update,
                (
                    sync_players_state,
                    interpolate_transforms,
                    camera_follow_local_player.after(sync_players_state),
                    draw_background_grid,
                )
                    .chain()
                    .run_if(in_state(AppState::InGame)),
            )
            .add_systems(
                FixedUpdate,
                (gather_and_store_inputs, broadcast_player_inputs)
                    .chain()
                    .run_if(in_state(AppState::InGame)),
            );
    }
}

// --- Ressources ---

#[derive(Resource)]
pub struct PlayerInputBuffer {
    pub history: [u8; 16],
}

impl Default for PlayerInputBuffer {
    fn default() -> Self {
        Self { history: [0; 16] }
    }
}

// --- Composants ---

#[derive(Component)]
pub struct MainCamera;

#[derive(Component)]
pub struct NetworkEntity(pub u32);

#[derive(Component)]
pub struct LocalPlayer;

#[derive(Component)]
pub struct TargetPosition(pub Vec2);

// --- Systèmes ---

fn setup_camera(mut commands: Commands) {
    commands.spawn((Camera2d, MainCamera));
}

fn sync_players_state(
    mut commands: Commands,
    mut events: MessageReader<NetworkSnapshotEvent>,
    mut q_players: Query<(Entity, &NetworkEntity, &mut TargetPosition)>,
    assets: Res<GameAssets>,
    session: Res<SessionData>,
) {
    for event in events.read() {
        let mut existing_entities: HashMap<u32, (Entity, Mut<TargetPosition>)> = q_players
            .iter_mut()
            .map(|(e, net_id, target)| (net_id.0, (e, target)))
            .collect();

        for server_player in &event.players {
            let pid = server_player.client_id.as_u32();

            if let Some((_, target_pos)) = existing_entities.get_mut(&pid) {
                target_pos.0 = Vec2::from(server_player.pos);
                existing_entities.remove(&pid);
            } else {
                let mut entity_cmds = commands.spawn((
                    Sprite {
                        image: assets.player_sprite.clone(),
                        color: if pid == session.custom_id {
                            Color::srgb(0.0, 1.0, 0.0)
                        } else {
                            Color::srgb(1.0, 0.0, 0.0)
                        },
                        custom_size: Some(Vec2::new(32.0, 32.0)),
                        ..default()
                    },
                    Transform::from_xyz(server_player.pos.0, server_player.pos.1, 0.0),
                    NetworkEntity(pid),
                    TargetPosition(Vec2::from(server_player.pos)),
                ));

                if pid == session.custom_id {
                    entity_cmds.insert(LocalPlayer);
                }
            }
        }

        for (entity, _) in existing_entities.values() {
            commands.entity(*entity).despawn();
        }
    }
}

fn interpolate_transforms(time: Res<Time>, mut query: Query<(&mut Transform, &TargetPosition)>) {
    let dt = time.delta_secs();
    let convergence_speed = 15.0;
    let lerp_factor = 1.0 - (-convergence_speed * dt).exp();

    for (mut transform, target) in &mut query {
        let target_vec3 = Vec3::new(target.0.x, target.0.y, transform.translation.z);

        if transform.translation.distance_squared(target_vec3) < 0.001 {
            transform.translation = target_vec3;
        } else {
            transform.translation = transform.translation.lerp(target_vec3, lerp_factor);
        }
    }
}

fn camera_follow_local_player(
    q_player: Query<&Transform, (With<LocalPlayer>, Without<MainCamera>)>,
    mut q_camera: Query<&mut Transform, With<MainCamera>>,
) {
    let Ok(player_transform) = q_player.single() else { return };
    let Ok(mut camera_transform) = q_camera.single_mut() else { return };

    camera_transform.translation.x = player_transform.translation.x;
    camera_transform.translation.y = player_transform.translation.y;
    println!("\r{:?}", camera_transform);
}

fn draw_background_grid(
    mut gizmos: Gizmos,
    q_camera: Query<&Transform, With<MainCamera>>,
) {
    let Ok(camera) = q_camera.single() else { return; };
    let cam_pos = camera.translation.truncate();

    let grid_size = 64.0;
    let extents = 1200.0;

    let start_x = ((cam_pos.x - extents) / grid_size).floor() * grid_size;
    let start_y = ((cam_pos.y - extents) / grid_size).floor() * grid_size;
    let end_x = start_x + extents * 2.0;
    let end_y = start_y + extents * 2.0;

    let grid_color = Color::srgba(1.0, 1.0, 1.0, 0.05);

    let mut x = start_x;
    while x <= end_x {
        gizmos.line_2d(Vec2::new(x, start_y), Vec2::new(x, end_y), grid_color);
        x += grid_size;
    }

    let mut y = start_y;
    while y <= end_y {
        gizmos.line_2d(Vec2::new(start_x, y), Vec2::new(end_x, y), grid_color);
        y += grid_size;
    }
}

fn gather_and_store_inputs(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut input_buffer: ResMut<PlayerInputBuffer>,
) {
    let mut current_frame_input = 0u8;

    if keyboard.pressed(KeyCode::KeyW) || keyboard.pressed(KeyCode::KeyZ) {
        current_frame_input |= INPUT_UP;
    }
    if keyboard.pressed(KeyCode::KeyS) {
        current_frame_input |= INPUT_DOWN;
    }
    if keyboard.pressed(KeyCode::KeyA) || keyboard.pressed(KeyCode::KeyQ) {
        current_frame_input |= INPUT_LEFT;
    }
    if keyboard.pressed(KeyCode::KeyD) {
        current_frame_input |= INPUT_RIGHT;
    }
    if keyboard.pressed(KeyCode::KeyE) {
        current_frame_input |= INPUT_ACTION;
    }

    input_buffer.history.copy_within(0..15, 1);
    input_buffer.history[0] = current_frame_input;
}

fn broadcast_player_inputs(
    input_buffer: Res<PlayerInputBuffer>,
    state: Res<ClientState>,
    session: Res<SessionData>,
) {
    let Some(conn) = &state.connection else { return };

    let sync_payload = PlayerInputPayload {
        inputs: input_buffer.history.map(|input| PlayerInput { input }),
    };

    let publish_packet = Publish {
        topic_id: CustomId::from(session.custom_id),
        payload: bitcode::encode(&sync_payload),
    };

    let stream = GameStream::new(
        LogicalStream::Input as u16,
        GameStreamReliability::Unreliable,
    );

    let _ = state.peer.send(conn, &stream, publish_packet.to_bytes());
}