use crate::config::ServerConfig;
use crate::core::AssignedShard;
use crate::player::Player;
use crate::states::ServerState;
use bevy::prelude::*;
use bytes::Bytes;
use serde::Serialize;
use shared::constants::STREAM_HEARTBEAT;
use shared::models::{AssignShard, ServerBinaryPacket, Status};
use shared::network::protocols::QuicBackend;
use shared::network::{GameConnection, GameNetworkEvent, GamePeer, GameStream, GameStreamReliability};
use std::net::SocketAddr;

pub struct OrchestratorPlugin;

impl Plugin for OrchestratorPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(HeartbeatTimer(Timer::from_seconds(5.0, TimerMode::Repeating)))
            .add_systems(Startup, connect_to_orchestrator)
            .add_systems(Update, (poll_orchestrator, send_heartbeat).chain());
    }
}

#[derive(Serialize)]
pub struct HeartbeatPayload {
    pub id: String,
    pub player_count: usize,
    pub max_players: usize,
    pub status: Status,
}

#[derive(Resource)]
pub struct OrchestratorClient {
    pub peer: GamePeer,
    pub connection: Option<GameConnection>,
}

#[derive(Resource)]
pub struct HeartbeatTimer(pub Timer);

fn connect_to_orchestrator(mut commands: Commands, config: Res<ServerConfig>) {
    let client_peer = GamePeer::new(QuicBackend::new());
    let addr = config.orchestrator_addr.parse::<SocketAddr>().expect("❌ IP:PORT invalide");

    client_peer.connect(&addr.ip().to_string(), addr.port()).unwrap_or_else(|e| {
        error!("⚠️ Impossible de se connecter à l'orchestrateur : {:?}", e);
    });

    commands.insert_resource(OrchestratorClient {
        peer: client_peer,
        connection: None,
    });

    info!("🚀 Client Orchestrateur initialisé vers : {}", addr);
}

fn poll_orchestrator(
    mut commands: Commands,
    mut orch_client_opt: Option<ResMut<OrchestratorClient>>,
    mut next_state: ResMut<NextState<ServerState>>,
) {
    let Some(mut orch_client) = orch_client_opt else { return };

    loop {
        match orch_client.peer.poll() {
            Ok(Some(event)) => match event {
                GameNetworkEvent::Connected(conn) => {
                    info!("🔗 Connecté à l'orchestrateur ! En attente d'assignation...");
                    orch_client.connection = Some(conn);
                    let _ = orch_client.peer.create_stream(conn, GameStreamReliability::Reliable);
                }
                GameNetworkEvent::Message { data, .. } => {
                    if data.is_empty() { continue; }

                    if data[0] == AssignShard::TAG {
                        if let Some(packet) = AssignShard::try_from_bytes(data) {
                            info!("🎯 Ordre reçu ! Ce serveur devient la Shard : {:?}", packet.shard_id);
                            commands.insert_resource(AssignedShard(packet.shard_id));
                            next_state.set(ServerState::Active);
                        }
                    }
                }
                GameNetworkEvent::Disconnected(_) => {
                    warn!("⚠️ Connexion à l'orchestrateur perdue.");
                    orch_client.connection = None;
                }
                _ => {}
            },
            Ok(None) => break,
            Err(e) => {
                error!("💥 Le thread réseau Orchestrateur a crashé : {:?}", e);
                let _ = orch_client.peer.shutdown();
                commands.remove_resource::<OrchestratorClient>();
                break;
            }
        }
    }
}

fn send_heartbeat(
    time: Res<Time>,
    state: Res<State<ServerState>>,
    mut timer: ResMut<HeartbeatTimer>,
    config: Res<ServerConfig>,
    player_query: Query<(), With<Player>>,
    orch_client_opt: Option<ResMut<OrchestratorClient>>,
) {
    let Some(orch_client) = orch_client_opt else { return };
    if !timer.0.tick(time.delta()).just_finished() { return; }

    let Some(conn) = orch_client.connection.as_ref() else { return; };

    let count = player_query.iter().count();
    let status = match state.get() {
        ServerState::WaitingAssignment => Status::Waiting,
        ServerState::Active => {
            if count >= config.max_players {
                Status::Full
            } else if count == 0 {
                Status::Empty
            } else {
                Status::Online
            }
        }
    };

    let payload = HeartbeatPayload {
        id: config.id.clone(),
        player_count: count,
        max_players: config.max_players,
        status,
    };

    if let Ok(json_bytes) = serde_json::to_vec(&payload) {
        let stream = GameStream::new(STREAM_HEARTBEAT, GameStreamReliability::Unreliable);
        if let Err(e) = orch_client.peer.send(conn, &stream, Bytes::from(json_bytes)) {
            error!("❌ Échec de l'envoi du heartbeat: {:?}", e);
        }
    }
}