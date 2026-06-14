mod db;
mod docker;

use db::{Database, ServerInfo};
use docker::DockerOrchestrator;
use serde::Deserialize;
use std::sync::Arc;
use std::time::Duration;
use std::collections::HashMap;
use uuid::Uuid;
use futures::StreamExt;

use network_protocol::network::protocols::QuicBackend;
use network_protocol::network::{GameNetworkEvent, GamePeer};
use internal_communication_protocol::internal_models::{STREAM_HEARTBEAT,Status,SpawnServer, AssignShard, ServerBinaryPacket};

#[derive(Deserialize)]
pub struct HeartbeatPayload {
    pub id: String,
    pub player_count: usize,
    pub max_players: usize,
    pub status: Status,
}

pub struct ServerConfig {
    pub image_name: String,
    pub max_players: usize,
    pub server_url: String,
}

impl ServerConfig {
    pub fn from_env() -> Self {
        Self {
            image_name: std::env::var("IMAGE_NAME")
                .unwrap_or_else(|_| "uqac_t3_mmo-server:local".to_string()),
            max_players: std::env::var("SERVER_MAX_PLAYERS")
                .unwrap_or_else(|_| "75".to_string())
                .parse()
                .expect("⚠️ SERVER_MAX_PLAYER doit être un nombre entier valide"),
            server_url: std::env::var("SERVER_URL")
                .unwrap_or_else(|_| "127.0.0.1".to_string()),
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let app_config = Arc::new(ServerConfig::from_env());
    let redis_url = std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379/".to_string());
    let min_available_servers: usize = std::env::var("MIN_AVAILABLE_SERVERS").unwrap_or(2.to_string()).parse()?;

    let database = Database::new(&redis_url).await?;
    println!("✅ Connecté à Redis !");

    let mut orchestrator_peer = GamePeer::new(QuicBackend::new());
    orchestrator_peer.listen("0.0.0.0", 4000)?;

    let docker_manager = Arc::new(DockerOrchestrator::new().await?);
    let mut autoscale_timer = tokio::time::interval(Duration::from_secs(1));
    let mut active_sessions: HashMap<Uuid, String> = HashMap::new();
    let mut server_streams: HashMap<Uuid, shared::network::GameStream> = HashMap::new();

    let mut conn = database.conn.clone();
    let _: () = redis::cmd("CONFIG").arg("SET").arg("notify-keyspace-events").arg("Ex").query_async(&mut conn).await?;
    let mut pubsub = database.client.get_async_pubsub().await?;
    pubsub.psubscribe("__keyevent@0__:expired").await?;

    let db_clone = database.clone();
    let docker_clone = docker_manager.clone();



    tokio::spawn(async move {
        let mut stream = pubsub.on_message();
        while let Some(msg) = stream.next().await {
            if let Ok(expired_key) = msg.get_payload::<String>() {
                if expired_key.starts_with("heartbeat:") {
                    let parts: Vec<&str> = expired_key.split(':').collect();
                    if parts.len() == 2 {
                        let server_id = parts[1];
                        println!("⏰ [TTL Redis] Le serveur {} n'a pas répondu. Destruction...", server_id);
                        let _ = docker_clone.remove_game_server(server_id).await;
                        let _ = db_clone.remove_server(server_id).await;
                    }
                }
            }
        }
    });

    loop {
        tokio::select! {
            _ = wait_for_shutdown() => {
                println!("Début du shutdown...");
                break;
            }

            _ = autoscale_timer.tick() => {
                if let Ok(servers) = database.get_all_servers().await {
                    let available_count = servers.iter().filter(|s| {
                        (s.status == Status::Waiting || s.status == Status::Starting)
                        && s.players_online < s.max_players
                    }).count();

                    if available_count < min_available_servers {
                        let to_spawn = min_available_servers - available_count;
                        println!("⚖️ [Auto-Scaler] {}/{} dispos. Spawn de {} instance(s)...", available_count, min_available_servers, to_spawn);

                        for _ in 0..to_spawn {
                            let server_id = Uuid::new_v4().to_string();
                            let docker_clone = docker_manager.clone();
                            let db_clone = database.clone();
                            let config_clone = app_config.clone();

                            tokio::spawn(async move {
                                match docker_clone.spawn_game_server(&server_id, &config_clone.image_name, config_clone.max_players).await {
                                    Ok(_) => {
                                        let server_info = ServerInfo {
                                            server_id,
                                            shard_id: None,
                                            players_online: 0,
                                            max_players: config_clone.max_players,
                                            status: Status::Starting,
                                        };
                                        let _ = db_clone.save_server(&server_info).await;
                                        println!("🚀 Instance lancée avec succès");
                                    },
                                    Err(e) => acronym_error("Spawn", e),
                                }
                            });
                        }
                    }

                    else if available_count > min_available_servers {
                        let to_kill = available_count - min_available_servers;
                        let empty_servers: Vec<_> = servers.iter().filter(|s| s.status == Status::Empty && s.players_online == 0).collect();
                        let kill_count = std::cmp::min(to_kill, empty_servers.len());

                        for server in empty_servers.into_iter().take(kill_count) {
                            let docker_clone = docker_manager.clone();
                            let db_clone = database.clone();
                            let server_id = server.server_id.clone();

                            tokio::spawn(async move {
                                if docker_clone.remove_game_server(&server_id).await.is_ok() {
                                    let _ = db_clone.remove_server(&server_id).await;
                                }
                            });
                        }
                    }
                }
            }

            _ = tokio::time::sleep(Duration::from_millis(10)) => {
                while let Ok(Some(event)) = orchestrator_peer.poll() {
                    match event {
                        GameNetworkEvent::StreamCreated(conn, stream) => {
                            if stream.is_reliable() {
                                server_streams.insert(conn.connection_id, stream);
                            }
                        }
                        GameNetworkEvent::Disconnected(conn) => {
                            if let Some(server_id) = active_sessions.remove(&conn.connection_id) {
                                let _ = docker_manager.remove_game_server(&server_id).await;
                                let _ = database.remove_server(&server_id).await;
                            }
                            server_streams.remove(&conn.connection_id); // Nettoyage
                        }
                        GameNetworkEvent::Message { connection, stream, data } => {
                            // Si le message provient du flux de Heartbeat, c'est un serveur de jeu qui se signale
                            if (stream.stream_id >> 2) == STREAM_HEARTBEAT {
                                if let Ok(json_str) = String::from_utf8(data.to_vec()) {
                                    if let Ok(payload) = serde_json::from_str::<HeartbeatPayload>(&json_str) {
                                        active_sessions.insert(connection.connection_id, payload.id.clone());

                                        if let Ok(servers) = database.get_all_servers().await {
                                            if let Some(existing) = servers.iter().find(|s| s.server_id == payload.id) {
                                                let updated_info = ServerInfo {
                                                    server_id: payload.id,
                                                    shard_id: existing.shard_id, // On conserve le ShardId s'il en a déjà un
                                                    players_online: payload.player_count,
                                                    max_players: payload.max_players,
                                                    status: payload.status,
                                                };
                                                let _ = database.save_server(&updated_info).await;
                                            }
                                        }
                                    }
                                }
                            }
                            // 🚀 NOUVEAU : Si c'est un flux classique (fiable), ça vient du Spatial Server !
                            else {
                                if data.is_empty() { continue; }
                                let tag = data[0];

                                // On intercepte le Tag SpawnServer
                                if tag == SpawnServer::TAG {
                                    if let Some(spawn_req) = SpawnServer::try_from_bytes(data) {
                                        // Extraction de l'ID (u32 ou CustomId selon ton modèle)
                                        let shard_raw_id: u32 = spawn_req.shard_id.into();
                                        println!("🚀 [Orchestrator] Requête de spawn reçue du SpatialServer pour la Shard : {}", shard_raw_id);

                                        if let Ok(servers) = database.get_all_servers().await {
                                            if let Some(chosen_server) = servers.iter().find(|s| s.status == Status::Waiting && s.shard_id.is_none()) {
                                                if let Some((&conn_uuid, _)) = active_sessions.iter().find(|(_, id)| **id == chosen_server.server_id) {

                                                    // 🚀 NOUVEAU : On récupère le flux fiable que le serveur a ouvert !
                                                    if let Some(out_stream) = server_streams.get(&conn_uuid) {

                                                        // ✅ CORRECTION : On utilise le modèle partagé au lieu de forger les bytes à la main !
                                                        let assign_packet = AssignShard {
                                                            shard_id: shared::custom_id::CustomId::from(shard_raw_id),
                                                        };

                                                        let target_server_conn = shared::network::GameConnection { connection_id: conn_uuid };

                                                        // On envoie directement le packet sérialisé par ta macro
                                                        if orchestrator_peer.send(&target_server_conn, out_stream, assign_packet.to_bytes()).is_ok() {
                                                            println!("🎯 [Orchestrator] Shard {} assignée avec succès au serveur conteneur {}", shard_raw_id, chosen_server.server_id);

                                                            let updated_info = ServerInfo {
                                                                server_id: chosen_server.server_id.clone(),
                                                                shard_id: Some(shard_raw_id),
                                                                players_online: chosen_server.players_online,
                                                                max_players: chosen_server.max_players,
                                                                status: Status::Online,
                                                            };
                                                            let _ = database.save_server(&updated_info).await;
                                                        }
                                                    } else {
                                                        println!("⚠️ [Orchestrator] Le serveur {} n'a pas encore de flux fiable ouvert.", chosen_server.server_id);
                                                    }
                                                }
                                            } else {
                                                todo!("Gérer pas assez de servers dans le pool")
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    if let Ok(servers) = database.get_all_servers().await {
        for server in servers {
            let _ = docker_manager.remove_game_server(&server.server_id).await;
            let _ = database.remove_server(&server.server_id).await;
        }
    }
    let _ = orchestrator_peer.shutdown();
    Ok(())
}

fn acronym_error(ctx: &str, e: anyhow::Error) {
    eprintln!("❌ [Docker {}] : {}", ctx, e);
}

async fn wait_for_shutdown() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let mut sigterm = signal(SignalKind::terminate()).expect("Impossible d'écouter SIGTERM");
        let mut sigint = signal(SignalKind::interrupt()).expect("Impossible d'écouter SIGINT");

        tokio::select! {
            _ = sigterm.recv() => { println!("\n🛑 SIGTERM (ex: Docker stop) reçu."); }
            _ = sigint.recv() => { println!("\n🛑 SIGINT (CTRL+C) reçu."); }
        }
    }

    #[cfg(windows)]
    {
        use tokio::signal::windows::{ctrl_c, ctrl_break};
        let mut ctrl_c_signal = ctrl_c().expect("Impossible d'écouter CTRL+C");
        let mut ctrl_break_signal = ctrl_break().expect("Impossible d'écouter CTRL+BREAK");

        tokio::select! {
            _ = ctrl_c_signal.recv() => { println!("\n🛑 Signal CTRL+C (Windows) reçu."); }
            _ = ctrl_break_signal.recv() => { println!("\n🛑 Signal CTRL+BREAK (Windows) reçu."); }
        }
    }

    #[cfg(not(any(unix, windows)))]
    {
        let _ = tokio::signal::ctrl_c().await;
        println!("\n🛑 Signal d'arrêt générique reçu.");
    }
}