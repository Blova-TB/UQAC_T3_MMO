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
use std::sync::Arc;
use std::time::Duration;
use std::collections::{HashMap, HashSet};
use uuid::Uuid;
use tokio::signal;
use futures::StreamExt;

#[derive(Deserialize)]
pub struct HeartbeatPayload {
    pub id: String,
    pub player_count: usize,
    pub max_players: usize,
    pub status: Status,
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

    let mut autoscale_timer = tokio::time::interval(Duration::from_secs(1));

    let mut active_sessions: HashMap<Uuid, String> = HashMap::new();

    println!("🛡️ L'orchestrateur (Auto-Scaling: {}) est en ligne. (CTRL+C pour quitter)", min_available_servers);

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
                    if parts.len() == 3 {
                        let server_id = parts[1];
                        let port = parts[2];
                        println!("⏰ [TTL Redis] Le serveur {} (Port {}) n'a pas donné de nouvelles depuis 15s. Extermination...", server_id, port);
                        let container_name = format!("game-shard-{}", port);
                        let _ = docker_clone.remove_game_server(&container_name).await;
                        let _ = db_clone.remove_server(server_id, port).await;
                    }
                }
            }
        }
    });

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
                        (s.status == Status::Starting || s.status == Status::Empty)
                        && s.players_online < s.max_players
                    }).count();

                    if available_count < min_available_servers {
                        let to_spawn = min_available_servers - available_count;
                        println!("⚖️ [Auto-Scaler] {}/{} serveurs dispos. Démarrage de {} instance(s)...",
                            available_count, min_available_servers, to_spawn);

                        let mut used_ports = HashSet::new();
                        for server in &servers {
                            if let Some(port_str) = server.address.split(':').last() {
                                if let Ok(p) = port_str.parse::<u16>() {
                                    used_ports.insert(p);
                                }
                            }
                        }
                        for _ in 0..to_spawn {
                            let mut selected_port = None;
                            for port in 4001..=5000 {
                                if !used_ports.contains(&port) {
                                    selected_port = Some(port);
                                    used_ports.insert(port);
                                    break;
                                }
                            }

                            if let Some(port) = selected_port {
                                let container_name = format!("game-shard-{}", port);

                                let docker_clone = docker_manager.clone();
                                let db_clone = database.clone();

                                tokio::spawn(async move {
                                    match docker_clone.spawn_game_server(&container_name, "uqac_t3_mmo-server:local", &port.to_string()).await {
                                        Ok((_, server_id)) => {
                                            let server_info = ServerInfo {
                                                container_id: server_id,
                                                address: format!("10.0.0.203:{}", port),
                                                players_online: 0,
                                                max_players: 100,
                                                status: Status::Starting,
                                            };
                                            let _ = db_clone.save_server(&server_info).await;
                                            println!("🚀 Instance '{}' lancée avec succès sur le port recyclé {}", container_name, port);
                                        },
                                        Err(e) => eprintln!("❌ Échec lancement {} : {}", container_name, e),
                                    }
                                });
                            } else {
                                eprintln!("🚨 [CRITIQUE] Aucun port UDP libre trouvé dans la plage 4001-5000 !");
                                break;
                            }
                        }
                    }
                    else if available_count > min_available_servers {
                        let to_kill = available_count - min_available_servers;

                        let empty_servers: Vec<_> = servers
                            .iter()
                            .filter(|s| s.status == Status::Empty && s.players_online == 0)
                            .collect();

                        let kill_count = std::cmp::min(to_kill, empty_servers.len());

                        if kill_count > 0 {
                            println!("⚖️ [Auto-Scaler] {}/{} serveurs dispos. Arrêt de {} instance(s) excédentaire(s)...",
                                available_count, min_available_servers, kill_count);

                            for server in empty_servers.into_iter().take(kill_count) {
                                if let Some(port_str) = server.address.split(':').last() {
                                    let container_name = format!("game-shard-{}", port_str);

                                    let docker_clone = docker_manager.clone();
                                    let db_clone = database.clone();
                                    let container_id = server.container_id.clone();
                                    let port_string = port_str.to_string();

                                    tokio::spawn(async move {
                                        match docker_clone.remove_game_server(&container_name).await {
                                            Ok(_) => {
                                                let _ = db_clone.remove_server(&container_id, &port_string).await;
                                                println!("🗑️ Instance excédentaire '{}' nettoyée avec succès.", container_name);
                                            }
                                            Err(e) => eprintln!("❌ Échec de l'arrêt de {} : {}", container_name, e),
                                        }
                                    });
                                }
                            }
                        }
                    }
                }
            }

            _ = tokio::time::sleep(Duration::from_millis(10)) => {
                while let Ok(Some(event)) = orchestrator_peer.poll() {
                    match event {
                        GameNetworkEvent::Message { connection, stream, data } => {
                            let raw_stream_id = stream.stream_id >> 2;

                            match raw_stream_id {
                                STREAM_HEARTBEAT => {
                                    if let Ok(json_str) = String::from_utf8(data.to_vec()) {
                                        if let Ok(payload) = serde_json::from_str::<HeartbeatPayload>(&json_str) {

                                            active_sessions.insert(connection.connection_id, payload.id.clone());

                                            if let Ok(servers) = database.get_all_servers().await {
                                                if let Some(existing) = servers.iter().find(|s| s.container_id == payload.id) {
                                                    let updated_info = ServerInfo {
                                                        container_id: payload.id.clone(),
                                                        address: existing.address.clone(),
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
                                _ => {}
                            }
                        }
                        GameNetworkEvent::Disconnected(conn) => {
                            println!("💔 Déconnexion QUIC détectée pour la connexion {:?}", conn.connection_id);

                            if let Some(server_id) = active_sessions.remove(&conn.connection_id) {
                                let db_clone = database.clone();
                                let docker_clone = docker_manager.clone();

                                tokio::spawn(async move {
                                    if let Ok(servers) = db_clone.get_all_servers().await {
                                        if let Some(server) = servers.iter().find(|s| s.container_id == server_id) {
                                            if let Some(port_str) = server.address.split(':').last() {
                                                let container_name = format!("game-shard-{}", port_str);
                                                let _ = docker_clone.remove_game_server(&container_name).await;
                                                let _ = db_clone.remove_server(&server_id, port_str).await;
                                            }
                                        }
                                    }
                                    println!("🧹 Nettoyage complet (Redis + Docker) terminé pour le serveur {}", server_id);
                                });
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    Ok(())
}