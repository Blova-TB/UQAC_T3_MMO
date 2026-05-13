mod player;
mod debug_tool;
mod network;

use player::PlayerPlugin;
use debug_tool::DebugToolPlugin;

use bevy::prelude::*;
use avian2d::prelude::*;
use bevy::app::ScheduleRunnerPlugin;
use std::time::Duration;
use bevy::scene::ScenePlugin;
use uuid::Uuid;
use std::env;
use shared::network::{GameConnection, GameNetworkEvent, GamePeer, GameStream, GameStreamReliability};
use shared::network::protocols::QuicBackend;
use std::collections::HashMap;
use std::net::UdpSocket;
use bitcode::{Decode, Encode};
use bytes::Bytes;
use serde::Serialize;

#[derive(Resource)]
pub struct ServerConfig {
    pub id: String,
    pub ip: String,
    pub port: u16,
    pub zone: String,
    pub max_players: usize,
    pub orchestrator_addr: String,
}

pub struct PlayerInfo {
    pub id: u64,
    pub username: String,
    pub entity: Entity,
    pub unreliable_stream: Option<GameStream>,
}

#[derive(Resource, Default)]
pub struct PlayerRegistry {
    pub players: HashMap<GameConnection, PlayerInfo>,
    next_id: u64,
}

#[derive(Resource)]
pub struct NetworkState {
    pub peer: GamePeer,
    pub heartbeat_socket: UdpSocket,
}


#[derive(Resource)]
pub struct HeartbeatTimer(Timer);

// --- Protocoles ---

// Protocole Binaire Client <-> Serveur Dédié
#[derive(Encode, Decode)]
pub enum ClientPacket {
    Join { username: String },
}

#[derive(Encode, Decode)]
pub enum ServerPacket {
    Welcome { player_id: u64 },
    RejectedFull,
}

// Protocole JSON Serveur Dédié -> Orchestrateur
#[derive(Serialize)]
pub struct HeartbeatPayload {
    pub id: String,
    pub ip: String,
    pub port: u16,
    pub zone: String,
    pub player_count: usize,
    pub max_players: usize,
    pub status: &'static str,
}

impl ServerConfig {
    pub fn from_env() -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            ip: env::var("DS_IP").unwrap_or_else(|_| "0.0.0.0".to_string()),
            port: env::var("DS_PORT")
                .unwrap_or_else(|_| "5000".to_string())
                .parse()
                .expect("DS_PORT doit être un entier valide"),
            zone: env::var("DS_ZONE").unwrap_or_else(|_| "global".to_string()),
            max_players: env::var("MAX_PLAYERS")
                .unwrap_or_else(|_| "100".to_string())
                .parse()
                .expect("MAX_PLAYERS doit être un entier valide"),
            orchestrator_addr: env::var("ORCHESTRATOR_ADDR")
                .unwrap_or_else(|_| "127.0.0.1:4000".to_string()),
        }
    }
}

fn main() {
    App::new()
        .add_plugins((
            MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(Duration::from_secs_f64(1.0 / 60.0))),
            AssetPlugin::default(),
            ScenePlugin,
            TransformPlugin,
            PhysicsPlugins::default(),
            //PlayerPlugin,
            //DebugToolPlugin,
        ))
        .insert_resource(ServerConfig::from_env())
        .init_resource::<PlayerRegistry>()
        .insert_resource(HeartbeatTimer(Timer::from_seconds(
            5.0,
            TimerMode::Repeating,
        )))
        .add_systems(Startup, bind_sockets)
        .add_systems(Update, (receive_packets, send_heartbeat, debug_print).chain())
        .insert_resource(Gravity(Vec2::ZERO))
        .run();
}

fn bind_sockets(mut commands: Commands, config: Res<ServerConfig>) {
    // 1. Socket de jeu (QUIC)
    let backend = QuicBackend::new();
    let peer = GamePeer::new(backend);
    peer.listen(&config.ip, config.port).unwrap_or_else(|e| {
        panic!("Échec du bind du Game Socket sur le port {}: {:?}", config.port, e);
    });

    // 2. Socket Heartbeat (UDP standard, non-bloquant)
    let hb_socket = UdpSocket::bind("0.0.0.0:0").expect("Échec du bind du socket Heartbeat");
    hb_socket.set_nonblocking(true).unwrap();

    commands.insert_resource(NetworkState {
        peer,
        heartbeat_socket: hb_socket,
    });

    println!(
        "Dédié [{}] en ligne. Écoute UDP(QUIC) sur {}:{}. Zone: {}",
        config.id, config.ip, config.port, config.zone
    );
}

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
                    // Ouverture immédiate d'un stream fiable pour les paquets système (Join/Welcome)
                    let _ = network.peer.create_stream(conn, GameStreamReliability::Reliable);
                }
                GameNetworkEvent::Disconnected(conn) => {
                    if let Some(session) = registry.players.remove(&conn) {
                        commands.entity(session.entity).despawn();
                    }
                }
                GameNetworkEvent::StreamCreated(conn, stream) => {
                    // Enregistrement du flux pour les paquets de position
                    if let Some(session) = registry.players.get_mut(&conn) {
                        if !stream.is_reliable() {
                            session.unreliable_stream = Some(stream);
                        }
                    }
                }
                GameNetworkEvent::Message { connection, stream, data } => {
                    if let Ok(packet) = bitcode::decode::<ClientPacket>(&data) {
                        match packet {
                            ClientPacket::Join { username } => {
                                if registry.players.len() >= config.max_players {
                                    let reject = bitcode::encode(&ServerPacket::RejectedFull);
                                    let _ = network.peer.send(&connection, &stream, Bytes::from(reject));
                                    continue;
                                }

                                registry.next_id += 1;
                                let new_id = registry.next_id;

                                // Instanciation de l'entité ECS (utilise le PlayerBundle défini précédemment)
                                let entity = commands
                                    .spawn(crate::player::PlayerBundle::new(
                                        Vec2::ZERO,
                                        100,
                                        300.0,
                                        15.0
                                    ))
                                    .id();

                                // Demande immédiate d'un flux non-fiable pour la physique
                                let _ = network.peer.create_stream(connection, GameStreamReliability::Unreliable);

                                registry.players.insert(
                                    connection,
                                    PlayerInfo {
                                        id: new_id,
                                        username,
                                        entity,
                                        unreliable_stream: None, // Sera populé par StreamCreated
                                    },
                                );

                                let welcome = bitcode::encode(&ServerPacket::Welcome {
                                    player_id: new_id,
                                });
                                let _ = network.peer.send(&connection, &stream, Bytes::from(welcome));
                            }
                        }
                    }
                }
                GameNetworkEvent::Error { connection, inner } => {
                    eprintln!("Network protocol error for {:?}: {:?}", connection.connection_id, inner);
                }
                GameNetworkEvent::StreamClosed(_conn, _stream) => {
                    println!("Stream closed for connection {:?}. TODO ??", _conn.connection_id);
                }
            },
            Ok(None) => break,
            Err(e) => {
                eprintln!("Erreur de polling GamePeer : {:?}", e);
                break;
            }
        }
    }
}

fn send_heartbeat(
    time: Res<Time>,
    mut timer: ResMut<HeartbeatTimer>,
    config: Res<ServerConfig>,
    registry: Res<PlayerRegistry>,
    network: Res<NetworkState>,
) {
    if timer.0.tick(time.delta()).just_finished() {
        let count = registry.players.len();
        let status = if count >= config.max_players {
            "FULL"
        } else {
            "ONLINE"
        };

        let payload = HeartbeatPayload {
            id: config.id.clone(),
            ip: config.ip.clone(),
            port: config.port,
            zone: config.zone.clone(),
            player_count: count,
            max_players: config.max_players,
            status,
        };

        if let Ok(json) = serde_json::to_string(&payload) {
            let _ = network
                .heartbeat_socket
                .send_to(json.as_bytes(), &config.orchestrator_addr);
        }
    }
}

fn debug_print(registry: Res<PlayerRegistry>) {
    println!("Players connected: {}", registry.players.len());
}