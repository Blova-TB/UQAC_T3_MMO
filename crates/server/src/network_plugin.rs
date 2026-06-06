use bevy::prelude::*;
use bevy::time::{Timer, TimerMode};
use rand::Rng;
use std::collections::HashMap;

use crate::{ServerState, orchestrator_plugin::AssignedShard};
use crate::config::ServerConfig;
use crate::player::Player;

use shared::game_protocol::{LogicalStream, PlayerData, WorldSyncPayload};
use shared::network::{GameConnection, GameNetworkEvent, GamePeer, GameStream, GameStreamReliability};
use shared::network::protocols::QuicBackend;

use shared::models::{
    BrokerHandshakeShard, ClientLeft, SpawnPlayerShard, PositionUpdate, Publish, ServerBinaryPacket,
    Vec2 as MathVec2, ServerHeartBeat
};
use shared::custom_id::CustomId;

pub struct NetworkServerPlugin;

#[derive(Resource, Default)]
pub struct ClientEntities(pub HashMap<u32, Entity>);

#[derive(Resource)]
pub struct BrokerConnection {
    pub peer: GamePeer,
    pub connection: Option<GameConnection>,
    pub topic: u32,
}

/// Identifie le client réseau lié à cette entité joueur
#[derive(Component)]
pub struct NetworkClient {
    pub id: CustomId,
}

/// Gère le timer individuel et désynchronisé (staggered) pour l'envoi au Spatial Server
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

impl Plugin for NetworkServerPlugin {
    fn build(&self, app: &mut App) {
        app
            .init_resource::<ClientEntities>()
            .add_systems(OnEnter(ServerState::Active), connect_to_broker)
            .add_systems(Update, (
                poll_broker,
                broadcast_sync_to_clients,
                send_spatial_updates,
                send_broker_heartbeat,
            ).chain().run_if(in_state(ServerState::Active)));
    }
}

fn connect_to_broker(
    mut commands: Commands,
    config: Res<ServerConfig>,
    assigned_shard: Res<AssignedShard>,
) {
    let peer = GamePeer::new(QuicBackend::new());

    let addr: std::net::SocketAddr = config.broker_addr.parse()
        .expect("❌ BROKER_ADDR invalide");

    peer.connect(&addr.ip().to_string(), addr.port()).unwrap_or_else(|e| {
        panic!("Échec de connexion au Broker sur {}: {:?}", config.broker_addr, e);
    });

    let shard_id_u32 = assigned_shard.0.as_u32();

    commands.insert_resource(BrokerConnection {
        peer,
        connection: None,
        topic: shard_id_u32,
    });

    println!("🔗 Shard assignée (ID: {}) tente de se connecter au Broker.", shard_id_u32);
}

fn poll_broker(
    mut broker: ResMut<BrokerConnection>,
    mut commands: Commands,
    mut client_entities: ResMut<ClientEntities>,
) {
    loop {
        match broker.peer.poll() {
            Ok(Some(event)) => match event {
                GameNetworkEvent::Connected(conn) => {
                    println!("✅ Connecté au Broker avec succès ! Demande de flux fiable pour Handshake...");
                    broker.connection = Some(conn);

                    if let Err(e) = broker.peer.create_stream(conn, GameStreamReliability::Reliable) {
                        eprintln!("❌ Échec lors de la demande de flux fiable : {:?}", e);
                    }
                }

                GameNetworkEvent::StreamCreated(conn, stream) => {
                    if stream.is_reliable() {
                        let handshake = BrokerHandshakeShard {
                            shard_id: CustomId::from(broker.topic),
                        };
                        let _ = broker.peer.send(&conn, &stream, handshake.to_bytes());
                        println!("🌍 Shard Handshake envoyé !");
                    }
                }

                GameNetworkEvent::Message { data, .. } => {
                    if data.is_empty() { return; }
                    let tag = data[0];

                    match tag {
                        SpawnPlayerShard::TAG => {
                            let Some(pkt) = SpawnPlayerShard::try_from_bytes(data) else { return; };

                            let client_id: u32 = pkt.client_id.into();
                            let bevy_pos = Vec2::new(pkt.pos.x, pkt.pos.y);

                            // Ajout des composants réseaux et de synchronisation au spawn
                            let entity = commands.spawn((
                                crate::player::PlayerBundle::new(
                                    bevy_pos,
                                    100,
                                    300.0,
                                    15.0
                                ),
                                NetworkClient { id: pkt.client_id },
                                SpatialSync::default(),
                            )).id();

                            client_entities.0.insert(client_id, entity);
                            println!("👤 [Shard] Autorité prise sur le Joueur {} en {:?} ! Entité {:?} créée.", client_id, bevy_pos, entity);
                        }

                        ClientLeft::TAG => {
                            let Some(pkt) = ClientLeft::try_from_bytes(data) else { return; };
                            let client_id: u32 = pkt.client_id.into();

                            if let Some(entity) = client_entities.0.remove(&client_id) {
                                commands.entity(entity).despawn();
                                println!("👋 [Shard] Joueur {} a quitté la partie !", client_id);
                            }
                        }

                        _ => {}
                    }
                }
                GameNetworkEvent::Disconnected(_) => {
                    println!("❌ Déconnecté du Broker !");
                    broker.connection = None;
                }

                _ => {}
            },
            Ok(None) => break,
            Err(e) => {
                eprintln!("Erreur de polling Broker : {:?}", e);
                break;
            }
        }
    }
}

/// Système 1 : Envoie la position de TOUS les joueurs au Broker pour Broadcast (Haute Fréquence / Chaque Frame)
fn broadcast_sync_to_clients(
    broker: Res<BrokerConnection>,
    query: Query<(&Transform, &NetworkClient), With<Player>>,
) {
    let Some(conn) = &broker.connection else { return };
    if query.is_empty() { return; }

    let mut entities_data = Vec::with_capacity(query.iter().len());
    for (transform, client) in query.iter() {
        let pos = transform.translation.truncate();
        let client_id: u32 = client.id.into();

        entities_data.push(PlayerData{
            client_id: CustomId::from(client_id),
            pos: (pos.x, pos.y),
        });
    }

    let sync_payload = WorldSyncPayload {
        entities: entities_data
    };

    let encoded_payload = bitcode::encode(&sync_payload);

    let publish_packet = Publish {
        topic_id: CustomId::from(broker.topic),
        payload: encoded_payload,
    };

    let stream_id = LogicalStream::WorldSync as u16;
    let stream = GameStream::new(stream_id, GameStreamReliability::Unreliable);

    let _ = broker.peer.send(conn, &stream, publish_packet.to_bytes());
}

/// Système 2 : Envoie la position individuelle au Spatial Server (Basse Fréquence lissée à ~1Hz par joueur)
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

fn send_broker_heartbeat(
    time: Res<Time>,
    mut heartbeat_timer: Local<Timer>,
    broker: Res<BrokerConnection>,
    config: Res<ServerConfig>,
    player_query: Query<Entity, With<Player>>,
) {
    if heartbeat_timer.duration() == std::time::Duration::ZERO {
        *heartbeat_timer = Timer::from_seconds(1.0, TimerMode::Repeating);
    }

    let Some(conn) = &broker.connection else { return };

    if heartbeat_timer.tick(time.delta()).just_finished() {
        let player_count = player_query.iter().count();

        let occupancy_percent = (player_count.saturating_mul(100) / config.max_players.max(1)).min(100) as u8;

        let packet = ServerHeartBeat {
            shard_id: CustomId::from(broker.topic),
            occupancy: occupancy_percent,
        };

        let stream = GameStream::new(0, GameStreamReliability::Unreliable);
        let _ = broker.peer.send(conn, &stream, packet.to_bytes());
    }
}