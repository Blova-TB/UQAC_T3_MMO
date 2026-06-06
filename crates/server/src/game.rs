use crate::broker_plugin::BrokerConnection;
use crate::core::ClientEntities;
use crate::events::BrokerEvent;
use crate::player::{Player, PlayerBundle};
use crate::states::ServerState;
use bevy::prelude::*;
use rand::Rng;
use shared::custom_id::CustomId;
use shared::game_protocol::{LogicalStream, PlayerData, WorldSyncPayload};
use shared::models::{PositionUpdate, Publish, ServerBinaryPacket, Vec2 as MathVec2};
use shared::network::{GameStream, GameStreamReliability};

pub struct GamePlugin;

impl Plugin for GamePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                handle_broker_events,
                broadcast_sync_to_clients,
                send_spatial_updates,
            )
                .chain()
                .run_if(in_state(ServerState::Active)),
        );
    }
}

#[derive(Component)]
pub struct NetworkClient {
    pub id: CustomId,
}

#[derive(Component)]
pub struct SpatialSync {
    pub timer: Timer,
}

impl Default for SpatialSync {
    fn default() -> Self {
        let mut timer = Timer::from_seconds(1.0, TimerMode::Repeating);
        let random_offset = rand::rng().random_range(0.0..1.0);
        timer.tick(std::time::Duration::from_secs_f32(random_offset));
        Self { timer }
    }
}

// ✨ Le composant public qui garde en mémoire ce que veut faire le joueur
#[derive(Component, Default)]
pub struct PlayerInputState {
    pub latest_input: u8,
}

fn handle_broker_events(
    mut commands: Commands,
    mut ev_broker: MessageReader<BrokerEvent>,
    mut client_entities: ResMut<ClientEntities>,
    mut q_inputs: Query<&mut PlayerInputState>,
) {
    for ev in ev_broker.read() {
        match ev {
            BrokerEvent::SpawnPlayer { client_id, pos } => {
                let id_u32: u32 = (*client_id).into();
                let entity = commands
                    .spawn((
                        PlayerBundle::new(*pos, 100, 300.0, 15.0),
                        NetworkClient { id: *client_id },
                        SpatialSync::default(),
                        PlayerInputState::default(), // On attache l'état vierge
                    ))
                    .id();

                client_entities.0.insert(id_u32, entity);
                info!("👤 Autorité prise sur le Joueur {} en {:?} !", id_u32, pos);
            }

            BrokerEvent::PlayerLeft { client_id } => {
                let id_u32: u32 = (*client_id).into();
                if let Some(entity) = client_entities.0.remove(&id_u32) {
                    commands.entity(entity).despawn();
                    info!("👋 Joueur {} a quitté la partie !", id_u32);
                }
            }

            BrokerEvent::PlayerInput { client_id, payload } => {
                let id_u32: u32 = (*client_id).into();
                if let Some(&entity) = client_entities.0.get(&id_u32) {
                    if let Ok(mut input_state) = q_inputs.get_mut(entity) {
                        input_state.latest_input = payload.inputs[0].input;
                    }
                }
            }
        }
    }
}

fn broadcast_sync_to_clients(
    broker: Res<BrokerConnection>,
    query: Query<(&Transform, &NetworkClient), With<Player>>,
) {
    let Some(conn) = &broker.connection else { return };
    if query.is_empty() { return; }

    let entities_data: Vec<PlayerData> = query
        .iter()
        .map(|(transform, client)| {
            let pos = transform.translation.truncate();
            PlayerData {
                client_id: client.id,
                pos: (pos.x, pos.y),
            }
        })
        .collect();

    let sync_payload = WorldSyncPayload { entities: entities_data };
    let publish_packet = Publish {
        topic_id: CustomId::from(broker.topic),
        payload: bitcode::encode(&sync_payload),
    };

    let stream = GameStream::new(LogicalStream::WorldSync as u16, GameStreamReliability::Unreliable);
    let _ = broker.peer.send(conn, &stream, publish_packet.to_bytes());
}

fn send_spatial_updates(
    time: Res<Time>,
    broker: Res<BrokerConnection>,
    mut query: Query<(&mut SpatialSync, &NetworkClient, &Transform), With<Player>>,
) {
    let Some(conn) = &broker.connection else { return };

    for (mut sync, client, transform) in query.iter_mut() {
        if sync.timer.tick(time.delta()).just_finished() {
            let pos = transform.translation.truncate();
            let packet = PositionUpdate {
                client_id: client.id,
                pos: MathVec2::new(pos.x, pos.y),
            };

            let stream = GameStream::new(0, GameStreamReliability::Unreliable);
            let _ = broker.peer.send(conn, &stream, packet.to_bytes());
        }
    }
}