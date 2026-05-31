use bevy::prelude::*;
use bitcode::{Decode, Encode};
use bytes::{Buf, BufMut, BytesMut};
use std::hash::{Hash, Hasher};
use std::collections::hash_map::DefaultHasher;
use crate::{ServerState, orchestrator_plugin::AssignedShard};

use shared::constants::STREAM_PHYSICS;
use crate::config::ServerConfig;
use shared::network::{GameConnection, GameNetworkEvent, GamePeer, GameStream, GameStreamReliability};
use shared::network::protocols::QuicBackend;
use crate::player::Player;

use shared::custom_id::{CustomId, IdType};

pub struct NetworkServerPlugin;

use std::collections::HashMap;
use rand::{random, Rng};

#[derive(Resource, Default)]
pub struct ClientEntities(pub HashMap<u32, Entity>);

impl Plugin for NetworkServerPlugin {
    fn build(&self, app: &mut App) {
        app
            .init_resource::<NetworkEncoder>()
            .init_resource::<ClientEntities>()
            .add_systems(OnEnter(ServerState::Active), connect_to_broker)
            .add_systems(Update, (
                poll_broker,
                publish_positions_to_broker,
            ).chain().run_if(in_state(ServerState::Active)));
    }
}

#[derive(Resource, Default)]
pub struct NetworkEncoder {
    pub buffer: bitcode::Buffer,
}

#[derive(Resource)]
pub struct BrokerConnection {
    pub peer: GamePeer,
    pub connection: Option<GameConnection>,
    pub topic: u32,
}

// ⚠️ N'oublie pas de supprimer ces structs locales une fois que tu auras
// complètement migré vers shared::models::ServerSyncMessage comme on l'a vu !
#[derive(Encode, Decode)]
pub struct ServerSyncMessage {
    pub players: Vec<PlayerPositionData>,
}

#[derive(Encode, Decode)]
pub struct PlayerPositionData {
    pub entity_bits: u64,
    pub position: [f32; 2],
}

fn connect_to_broker(
    mut commands: Commands,
    config: Res<ServerConfig>,
    assigned_shard: Res<AssignedShard>, // <-- La ShardId fournie par l'Orchestrateur
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
    config: Res<ServerConfig>,
    mut commands: Commands,
    mut client_entities: ResMut<ClientEntities>,
) {
    loop {
        match broker.peer.poll() {
            Ok(Some(event)) => match event {
                // ÉTAPE 1 : La connexion brute est établie
                GameNetworkEvent::Connected(conn) => {
                    println!("✅ Connecté au Broker avec succès ! Demande de flux fiable pour Handshake...");
                    broker.connection = Some(conn);

                    // On ordonne à la librairie d'ouvrir un canal garanti
                    if let Err(e) = broker.peer.create_stream(conn, GameStreamReliability::Reliable) {
                        eprintln!("❌ Échec lors de la demande de flux fiable : {:?}", e);
                    }
                }

                // ÉTAPE 2 : Le canal réseau est prêt, on envoie l'identité
                GameNetworkEvent::StreamCreated(conn, stream) => {
                    if stream.is_reliable() {
                        println!("✅ Flux fiable ouvert. Envoi du Handshake de la Shard...");

                        // Construction du Tag 0x00
                        // 1 (Tag) + 1 (is_shard) + 4 (Topic/ShardId) = 6 octets ! (Au lieu de 38)
                        let mut handshake_msg = BytesMut::with_capacity(6);
                        handshake_msg.put_u8(0x00); // Tag 0x00 : Handshake
                        handshake_msg.put_u8(0x01); // is_shard = 1 (true)
                        handshake_msg.put_u32_le(broker.topic); // Le ShardId comme topic unique

                        // Envoi sécurisé
                        if let Err(e) = broker.peer.send(&conn, &stream, handshake_msg.freeze().into()) {
                            eprintln!("❌ Échec de l'envoi du Handshake Shard : {:?}", e);
                        } else {
                            println!("🌍 Shard authentifiée auprès du Broker sur le ShardId {}", broker.topic);
                        }
                    }
                }

                // ÉTAPE 3 : Gestion des messages venant du Broker
                GameNetworkEvent::Message { mut data, .. } => {
                    if data.is_empty() { return; }
                    let tag = data.get_u8();

                    match tag {
                        // TAG 0x06 : Un client a rejoint la zone de cette Shard
                        0x06 => {
                            if data.remaining() < 4 { return; }
                            let client_id = data.get_u32_le();

                            // On spawn l'entité du joueur dans le monde physique de Bevy
                            let entity = commands.spawn(crate::player::PlayerBundle::new(
                                Vec2::ZERO, 100, 300.0, 15.0
                            )).id();

                            client_entities.0.insert(client_id, entity);
                            println!("👤 [Shard] Joueur {} a rejoint la partie ! Entité {:?} créée.", client_id, entity);
                        }

                        // TAG 0x07 : Un client a quitté la zone (ou s'est déconnecté)
                        0x07 => {
                            if data.remaining() < 4 { return; }
                            let client_id = data.get_u32_le();

                            if let Some(entity) = client_entities.0.remove(&client_id) {
                                commands.entity(entity).despawn();
                                println!("👋 [Shard] Joueur {} a quitté la partie ! Entité {:?} détruite.", client_id, entity);
                            }
                        }

                        // TAG 0x05 : Inputs du joueur
                        0x05 => {
                            // En attente d'implémentation
                        }

                        _ => {}
                    }
                }

                // ÉTAPE 4 : Gestion des déconnexions
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

fn publish_positions_to_broker(
    broker: Res<BrokerConnection>,
    mut encoder: ResMut<NetworkEncoder>,
    query: Query<(Entity, &Transform), With<Player>>,
) {
    let Some(conn) = &broker.connection else { return };

    let mut players_data = Vec::with_capacity(query.iter().len());
    for (entity, transform) in query.iter() {
        players_data.push(PlayerPositionData {
            entity_bits: entity.to_bits(),
            position: transform.translation.truncate().to_array(),
        });
    }

    if players_data.is_empty() { return; }

    let sync_packet = ServerSyncMessage { players: players_data };
    let inner_payload = encoder.buffer.encode(&sync_packet);

    let payload_len = inner_payload.len() as u16;

    // 1 (Tag) + 4 (Topic) + 2 (Len) + X (Payload)
    let mut msg = BytesMut::with_capacity(7 + inner_payload.len());

    // Tag (0x03)
    msg.put_u8(0x03);
    // Topic (u32 Little Endian)
    msg.put_u32_le(broker.topic);
    // Payload Len (u16 Little Endian)
    msg.put_u16_le(payload_len);
    // Payload ([u8])
    msg.put_slice(inner_payload);

    let stream = GameStream::new(STREAM_PHYSICS, GameStreamReliability::Unreliable);
    let _ = broker.peer.send(conn, &stream, msg.freeze());
}