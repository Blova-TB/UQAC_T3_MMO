//
mod db;
mod docker;

use db::{Database, ServerInfo, Status};
use docker::DockerOrchestrator;
use shared::network::protocols::QuicBackend;
use shared::network::{GameNetworkEvent, GamePeer};
use shared::constants::{STREAM_HEARTBEAT};
use std::time::Duration;
use tokio::signal;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let redis_url = std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379/".to_string());
    println!("Connexion à Redis sur {}...", redis_url);
    let database = Database::new(&redis_url).await?;
    println!("✅ Connecté à Redis !");

    let mut orchestrator_peer = GamePeer::new(QuicBackend::new());
    orchestrator_peer.listen("0.0.0.0", 4000)?;
    println!("🎧 Orchestrateur en écoute (QUIC) sur 0.0.0.0:4000");

    let orchestrator = DockerOrchestrator::new().await?;
    let image_name = "uqac_t3_mmo-server:local";
    let container_name = "game-shard-01";
    let external_port = "4001";

    println!("⏳ Lancement de l'instance '{}'...", container_name);

    // 🌟 On déstructure le retour pour récupérer notre `server_id`
    match orchestrator.spawn_game_server(container_name, image_name, external_port).await {
        Ok((docker_id, server_id)) => {
            println!("✅ Serveur en ligne ! Docker ID : {} | Server ID : {}", &docker_id[..12], server_id);

            // On utilise l'ID généré par l'orchestrateur comme identifiant principal
            let server_info = ServerInfo {
                container_id: server_id.clone(),
                // L'adresse publique pour les joueurs (à modifier par ton IP publique en Prod)
                address: format!("127.0.0.1:{}", external_port),
                players_online: 0,
                max_players: 100,
                status: Status::Starting,
            };

            database.save_server(&server_info).await?;
            println!("💾 Instance '{}' enregistrée en BDD avec l'ID : {}", container_name, server_id);
        },
        Err(e) => eprintln!("❌ Échec lors du lancement du serveur : {}", e),
    }

    println!("🛡️ L'orchestrateur tourne. En attente des heartbeats... (CTRL+C pour quitter)");

    loop {
        tokio::select! {
            _ = signal::ctrl_c() => {
                println!("\n🛑 Signal d'arrêt reçu, fermeture de l'orchestrateur...");
                if let Err(e) = orchestrator_peer.shutdown() {
                    eprintln!("⚠️ Erreur lors de l'arrêt du réseau : {}", e);
                }
                break;
            }

            _ = tokio::time::sleep(Duration::from_millis(10)) => {
                while let Ok(Some(event)) = orchestrator_peer.poll() {
                    handle_network_event(event);
                }
            }
        }
    }

    Ok(())
}

fn handle_network_event(event: GameNetworkEvent) {
    match event {
        GameNetworkEvent::Connected(conn) => println!("🤝 Nouveau serveur connecté : {:?}", conn.connection_id),
        GameNetworkEvent::Disconnected(conn) => println!("💔 Serveur déconnecté : {:?}", conn.connection_id),
        GameNetworkEvent::Message { connection, stream, data } => {
            let raw_stream_id = stream.stream_id >> 2;

            match raw_stream_id {
                STREAM_HEARTBEAT => {
                    if let Ok(json_str) = String::from_utf8(data.to_vec()) {
                        // Le json_str contiendra le même "SERVER_ID" que celui généré au-dessus !
                        println!("💓 Heartbeat de [{}] : {}", connection.connection_id, json_str);
                        // Traitement BDD ici...
                    }
                }
                _ => {
                    println!("📦 Paquet non géré reçu sur le stream ID : {}", raw_stream_id);
                }
            }
        }
        GameNetworkEvent::Error { connection, inner } => eprintln!("⚠️ Erreur réseau avec [{}] : {}", connection.connection_id, inner),
        GameNetworkEvent::StreamCreated(conn, stream) => {
            let raw_stream_id = stream.stream_id >> 2;

            match raw_stream_id {
                STREAM_HEARTBEAT => println!("📡 Canal Heartbeat ouvert pour {}", conn.connection_id),
                _ => println!("🌊 Stream inconnu ({}) ouvert pour {}", raw_stream_id, conn.connection_id),
            }
        }
        GameNetworkEvent::StreamClosed(conn, stream) => println!("🥀 Stream {} fermé pour {}", stream.stream_id, conn.connection_id),
    }
}