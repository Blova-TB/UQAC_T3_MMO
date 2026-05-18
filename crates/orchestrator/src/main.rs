//
mod db;
mod docker;

use db::{Database, ServerInfo};
use docker::DockerOrchestrator;
use serde::Deserialize;
use shared::network::protocols::QuicBackend;
use shared::network::{GameNetworkEvent, GamePeer};
use shared::constants::STREAM_HEARTBEAT;
use shared::models::Status;
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::signal;

#[derive(Deserialize)]
pub struct HeartbeatPayload {
    pub id: String,
    pub player_count: usize,
    pub max_players: usize,
    pub status: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let redis_url = std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379/".to_string());
    let min_available_servers: usize = std::env::var("MIN_AVAILABLE_SERVERS")
        .unwrap_or_else(|_| "2".to_string())
        .parse()
        .expect("MIN_AVAILABLE_SERVERS doit être un entier");

    let database = Database::new(&redis_url).await?;
    println!("✅ Connecté à Redis !");

    let mut orchestrator_peer = GamePeer::new(QuicBackend::new());
    orchestrator_peer.listen("0.0.0.0", 4000)?;
    println!("🎧 Orchestrateur en écoute (QUIC) sur 0.0.0.0:4000");

    let docker_manager = Arc::new(DockerOrchestrator::new().await?);

    let next_port = Arc::new(AtomicU16::new(4001));

    let mut autoscale_timer = tokio::time::interval(Duration::from_secs(10));

    println!("🛡️ L'orchestrateur (Auto-Scaling: {}) est en ligne. (CTRL+C pour quitter)", min_available_servers);

    loop {
        tokio::select! {
            _ = signal::ctrl_c() => {
                println!("\n🛑 Signal d'arrêt reçu, fermeture...");
                let _ = orchestrator_peer.shutdown();
                break;
            }

            _ = autoscale_timer.tick() => {
                if let Ok(servers) = database.get_all_servers().await {

                    let available_count = servers.iter().filter(|s| {
                        (s.status == Status::Starting || s.status == Status::Online)
                        && s.players_online < s.max_players
                    }).count();

                    if available_count < min_available_servers {
                        let to_spawn = min_available_servers - available_count;
                        println!("⚖️ [Auto-Scaler] {}/{} serveurs dispos. Démarrage de {} instance(s)...",
                            available_count, min_available_servers, to_spawn);

                        for _ in 0..to_spawn {
                            let port = next_port.fetch_add(1, Ordering::SeqCst);
                            let container_name = format!("game-shard-{}", port);

                            let docker_clone = docker_manager.clone();
                            let db_clone = database.clone();

                            tokio::spawn(async move {
                                match docker_clone.spawn_game_server(&container_name, "uqac_t3_mmo-server:local", &port.to_string()).await {
                                    Ok((_, server_id)) => {
                                        let server_info = ServerInfo {
                                            container_id: server_id,
                                            address: format!("127.0.0.1:{}", port),
                                            players_online: 0,
                                            max_players: 100,
                                            status: Status::Starting,
                                        };
                                        let _ = db_clone.save_server(&server_info).await;
                                        println!("🚀 Instance '{}' lancée sur le port {}", container_name, port);
                                    },
                                    Err(e) => eprintln!("❌ Échec lancement {} : {}", container_name, e),
                                }
                            });
                        }
                    }
                }
            }

            _ = tokio::time::sleep(Duration::from_millis(10)) => {
                while let Ok(Some(event)) = orchestrator_peer.poll() {
                    match event {
                        GameNetworkEvent::Message { connection, stream, data } => {
                            if (stream.stream_id >> 2) == STREAM_HEARTBEAT {
                                if let Ok(json_str) = String::from_utf8(data.to_vec()) {

                                    if let Ok(payload) = serde_json::from_str::<HeartbeatPayload>(&json_str) {
                                        let server_status = if payload.status == "FULL" { Status::Full } else { Status::Online };

                                        if let Ok(servers) = database.get_all_servers().await {
                                            if let Some(existing) = servers.iter().find(|s| s.container_id == payload.id) {
                                                let updated_info = ServerInfo {
                                                    container_id: payload.id.clone(),
                                                    address: existing.address.clone(),
                                                    players_online: payload.player_count,
                                                    max_players: payload.max_players,
                                                    status: server_status,
                                                };
                                                let _ = database.save_server(&updated_info).await;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        GameNetworkEvent::Disconnected(conn) => {
                            println!("💔 Serveur déconnecté : {:?}", conn.connection_id);
                            // TODO (Optionnel) : Ajouter une logique pour retirer le serveur de Redis s'il se déconnecte définitivement
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    Ok(())
}