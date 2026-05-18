use std::collections::HashMap;
use std::env;
use bevy::prelude::*;
use bitcode::{Decode, Encode};
use bytes::Bytes;
use uuid::Uuid;
use shared::constants::STREAM_PHYSICS;
use crate::config::ServerConfig;

// Assure-toi que ces imports correspondent à l'arborescence de ton projet
use shared::network::{GameConnection, GameNetworkEvent, GamePeer, GameStream, GameStreamReliability};
use shared::network::protocols::QuicBackend;
use crate::player::Player;

pub struct NetworkServerPlugin;

impl Plugin for NetworkServerPlugin {
    fn build(&self, app: &mut App) {
        app
            .init_resource::<PlayerRegistry>()
            .init_resource::<NetworkEncoder>()
            // On initialise uniquement l'écoute pour les joueurs
            .add_systems(Startup, bind_server_socket)
            // On traite les paquets joueurs et on envoie les positions
            .add_systems(Update, (
                receive_packets,
                broadcast_positions,
            ).chain());
    }
}

// --- Ressources ---

#[derive(Resource, Default)]
pub struct NetworkEncoder {
    pub buffer: bitcode::Buffer,
}

#[derive(Resource, Default)]
pub struct PlayerRegistry {
    pub players: HashMap<GameConnection, PlayerInfo>,
    pub next_id: u64,
}

pub struct PlayerInfo {
    pub id: u64,
    pub username: String,
    pub entity: Entity,
}

#[derive(Resource)]
pub struct NetworkState {
    pub peer: GamePeer,
}

// --- Protocoles (Packets) ---

#[derive(Encode, Decode)]
pub struct ServerSyncMessage {
    pub players: Vec<PlayerPositionData>,
}

#[derive(Encode, Decode)]
pub struct PlayerPositionData {
    pub entity_bits: u64,
    pub position: [f32; 2],
}

#[derive(Encode, Decode)]
pub enum ClientPacket {
    Join { username: String },
}

#[derive(Encode, Decode)]
pub enum ServerPacket {
    Welcome { player_id: u64 },
    RejectedFull,
    SyncPositions(Vec<PlayerPositionData>),
}

// --- Systèmes ---

/// Initialise l'écouteur QUIC pour accepter les connexions des joueurs
fn bind_server_socket(mut commands: Commands, config: Res<ServerConfig>) {
    let server_backend = QuicBackend::new();
    let server_peer = GamePeer::new(server_backend);

    server_peer.listen("0.0.0.0", 4000).unwrap_or_else(|e| {
        panic!("Échec du bind du Game Socket sur le port 4000: {:?}", e);
    });

    commands.insert_resource(NetworkState { peer: server_peer });

    println!(
        "🎮 Serveur Joueurs en ligne. Écoute UDP(QUIC) sur 0.0.0.0:4000.",
    );
}

/// Écoute et traite tous les messages en provenance des joueurs
fn receive_packets(
    mut commands: Commands,
    mut network: ResMut<NetworkState>,
    mut registry: ResMut<PlayerRegistry>,
    config: Res<ServerConfig>,
) {
    loop {
        match network.peer.poll() {
            Ok(Some(event)) => match event {
                GameNetworkEvent::Connected(conn) => {
                    println!("👤 Nouveau joueur connecté : {:?}", conn.connection_id);
                    // On demande la création d'un flux fiable pour les commandes critiques
                    let _ = network.peer.create_stream(conn, GameStreamReliability::Reliable);
                }
                GameNetworkEvent::Disconnected(conn) => {
                    println!("👋 Joueur déconnecté : {:?}", conn.connection_id);
                    if let Some(session) = registry.players.remove(&conn) {
                        commands.entity(session.entity).despawn();
                    }
                }
                GameNetworkEvent::Message { connection, stream, data } => {
                    if let Ok(packet) = bitcode::decode::<ClientPacket>(&data) {
                        match packet {
                            ClientPacket::Join { username } => {
                                if registry.players.contains_key(&connection) {
                                    println!("⚠️ Le client {:?} a tenté un double Join.", connection.connection_id);
                                    continue;
                                }

                                if registry.players.len() >= config.max_players {
                                    let reject = bitcode::encode(&ServerPacket::RejectedFull);
                                    let _ = network.peer.send(&connection, &stream, Bytes::from(reject));
                                    continue;
                                }

                                registry.next_id += 1;
                                let new_id = registry.next_id;

                                // Instanciation de l'entité ECS du joueur
                                let entity = commands
                                    .spawn(crate::player::PlayerBundle::new(
                                        Vec2::ZERO,
                                        100,
                                        300.0,
                                        15.0
                                    ))
                                    .id();

                                // On demande la création d'un flux non-fiable pour sa physique
                                let _ = network.peer.create_stream(connection, GameStreamReliability::Unreliable);

                                registry.players.insert(
                                    connection,
                                    PlayerInfo {
                                        id: new_id,
                                        username,
                                        entity,
                                    },
                                );

                                let welcome = bitcode::encode(&ServerPacket::Welcome {
                                    player_id: new_id,
                                });
                                let _ = network.peer.send(&connection, &stream, Bytes::from(welcome));

                                println!("✅ Joueur {} a rejoint la partie !", new_id);
                            }
                        }
                    }
                }
                GameNetworkEvent::Error { connection, inner } => {
                    eprintln!("❌ Erreur réseau avec le client {:?}: {:?}", connection.connection_id, inner);
                }
                _ => {}
            },
            Ok(None) => break,
            Err(e) => {
                eprintln!("Erreur de polling GamePeer (Joueurs) : {:?}", e);
                break;
            }
        }
    }
}

/// Envoie les positions de tous les joueurs à tous les clients connectés
fn broadcast_positions(
    transport: Res<NetworkState>,
    clients: Res<PlayerRegistry>,
    mut encoder: ResMut<NetworkEncoder>,
    query: Query<(Entity, &Transform), With<Player>>,
) {
    if clients.players.is_empty() {
        return;
    }

    let mut players_data = Vec::with_capacity(query.iter().len());

    for (entity, transform) in query.iter() {
        players_data.push(PlayerPositionData {
            entity_bits: entity.to_bits(),
            position: transform.translation.truncate().to_array(),
        });
    }

    let sync_packet = ServerPacket::SyncPositions(players_data);
    let encoded_data = encoder.buffer.encode(&sync_packet);
    let serialized_bytes = Bytes::copy_from_slice(encoded_data);

    // On utilise un flux statique non-fiable pour l'envoi de la physique (Méthode de tes collègues)
    let physics_stream = GameStream::new(STREAM_PHYSICS, GameStreamReliability::Unreliable);

    for (conn, _) in clients.players.iter() {
        if let Err(e) = transport.peer.send(conn, &physics_stream, serialized_bytes.clone()) {
            eprintln!("Erreur d'envoi de position vers {:?}: {:?}", conn.connection_id, e);
        }
    }
}