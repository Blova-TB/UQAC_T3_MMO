//TODO si on a le temps, un système de reconnexion si l'orchestrator crash
use bevy::prelude::*;
use bytes::Bytes;
use serde::Serialize;
use std::net::SocketAddr;

use shared::network::{GameConnection, GameNetworkEvent, GamePeer, GameStream, GameStreamReliability};
use shared::network::protocols::QuicBackend;
use shared::constants::STREAM_HEARTBEAT;

use crate::player::Player;
use crate::config::ServerConfig;

pub struct OrchestratorPlugin;

impl Plugin for OrchestratorPlugin {
    fn build(&self, app: &mut App) {
        app
            .insert_resource(HeartbeatTimer(Timer::from_seconds(
                5.0,
                TimerMode::Repeating,
            )))
            .add_systems(Startup, connect_to_orchestrator)
            .add_systems(Update, (
                poll_orchestrator,
                send_heartbeat,
            ).chain());
    }
}

// 🌟 Payload allégé : L'orchestrateur n'a besoin que de savoir "Qui es-tu ?" et "Combien de joueurs as-tu ?"
#[derive(Serialize)]
pub struct HeartbeatPayload {
    pub id: String,
    pub player_count: usize,
    pub max_players: usize,
    pub status: &'static str,
}

#[derive(Resource)]
pub struct OrchestratorClient {
    pub peer: GamePeer,
    pub connection: Option<GameConnection>,
}

#[derive(Resource)]
pub struct HeartbeatTimer(pub Timer);

fn connect_to_orchestrator(mut commands: Commands, config: Res<ServerConfig>) {
    let client_backend = QuicBackend::new();
    let client_peer = GamePeer::new(client_backend);

    let addr = config.orchestrator_addr.parse::<SocketAddr>()
        .expect("❌ L'adresse de l'orchestrateur est mal formatée (attendu: IP:PORT)");

    let orch_ip = addr.ip().to_string();
    let orch_port = addr.port();

    client_peer.connect(&orch_ip, orch_port).unwrap_or_else(|e| {
        eprintln!("⚠️ Impossible d'initier la connexion à l'orchestrateur : {:?}", e);
    });

    commands.insert_resource(OrchestratorClient {
        peer: client_peer,
        connection: None,
    });

    println!("🚀 Client Orchestrateur initialisé vers : {}:{}", orch_ip, orch_port);
}

fn poll_orchestrator(mut commands: Commands, mut orch_client_opt: Option<ResMut<OrchestratorClient>>) {
    let Some(mut orch_client) = orch_client_opt else { return };

    loop {
        match orch_client.peer.poll() {
            Ok(Some(event)) => match event {
                GameNetworkEvent::Connected(conn) => {
                    println!("🔗 Connecté à l'orchestrateur ! Session ID: {:?}", conn.connection_id);
                    orch_client.connection = Some(conn);
                }
                GameNetworkEvent::Disconnected(_) => {
                    println!("⚠️ Connexion à l'orchestrateur perdue.");
                    orch_client.connection = None;
                }
                GameNetworkEvent::Error { inner, .. } => {
                    eprintln!("❌ Erreur avec l'orchestrateur : {:?}", inner);
                }
                _ => {}
            },
            Ok(None) => break,
            Err(e) => {
                eprintln!("💥 Le thread réseau a crashé : {:?}", e);
                let _ = orch_client.peer.shutdown();
                commands.remove_resource::<OrchestratorClient>();
                break;
            }
        }
    }
}

fn send_heartbeat(
    time: Res<Time>,
    mut timer: ResMut<HeartbeatTimer>,
    config: Res<ServerConfig>,
    player_query: Query<Entity, With<Player>>,
    mut orch_client_opt: Option<ResMut<OrchestratorClient>>,
) {
    let Some(mut orch_client) = orch_client_opt else { return };

    if !timer.0.tick(time.delta()).just_finished() {
        return;
    }

    let conn = orch_client.connection.as_ref().unwrap();

    let count = player_query.iter().count();
    let status = if count >= config.max_players { "FULL" } else { "ONLINE" };

    let payload = HeartbeatPayload {
        id: config.id.clone(),
        player_count: count,
        max_players: config.max_players,
        status,
    };

    if let Ok(json_bytes) = serde_json::to_vec(&payload) {
        let bytes_data = Bytes::from(json_bytes);
        let heartbeat_stream = GameStream::new(STREAM_HEARTBEAT, GameStreamReliability::Unreliable);

        if let Err(e) = orch_client.peer.send(conn, &heartbeat_stream, bytes_data) {
            eprintln!("❌ Échec de l'envoi du heartbeat: {:?}", e);
        } else {
            println!("💓 Heartbeat expédié ({} joueurs)", count);
        }
    }
}