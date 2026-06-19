use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use rand::Rng;
use std::time::Duration;
use tokio::sync::watch;
use tokio::time;

use client_communication_protocol::client_models::*;
use custom_id::custom_id::CustomId;
use internal_communication_protocol::internal_models::{
    Broadcast, BrokerHandshakeClient, Publish, RefuseClient, ServerBinaryPacket,
};
use network_protocol::network::protocols::QuicBackend;
use network_protocol::network::{GameNetworkEvent, GamePeer, GameStream, GameStreamReliability};

// --- CONFIGURATION ---
const GATEKEEPER_URL: &str = "http://127.0.0.1:3000";
const TICK_RATE_HZ: u64 = 30; // Les bots peuvent envoyer leurs inputs à 30Hz pour économiser le CPU
const MAP_MIN: f32 = 1000.0; // Marge de sécurité par rapport au bord 0
const MAP_MAX: f32 = 99000.0; // Marge de sécurité par rapport au bord 100000

#[tokio::main]
async fn main() {
    // Lecture des arguments (ex: cargo run -- 100 50)
    // arg 1 : num_bots (défaut 10)
    // arg 2 : délai de spawn en millisecondes (défaut 50ms)
    let args: Vec<String> = std::env::args().collect();
    let num_bots: usize = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(10);
    let spawn_delay_ms: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(50);

    println!("🤖 Démarrage de la simulation avec {} bots...", num_bots);
    println!("⏱️  Ramp-up : 1 bot toutes les {} ms", spawn_delay_ms);

    let (stop_tx, stop_rx) = watch::channel(false);
    let mut handles = Vec::new();

    for i in 1..=num_bots {
        let rx = stop_rx.clone();

        handles.push(tokio::spawn(async move {
            run_bot(i, rx).await;
        }));

        // 🛡️ L'ÉTALEMENT DES CONNEXIONS EST ICI
        // On attend un peu avant de spawner le bot suivant pour lisser la charge CPU et Réseau
        if spawn_delay_ms > 0 {
            time::sleep(Duration::from_millis(spawn_delay_ms)).await;
        }
    }

    println!("✅ Tous les bots sont lancés ! En attente du signal d'arrêt (Ctrl+C)...");

    tokio::signal::ctrl_c().await.unwrap();
    println!("\n🛑 Signal d'arrêt reçu. Arrêt des bots en cours...");
    let _ = stop_tx.send(true);

    for handle in handles {
        let _ = handle.await;
    }
    println!("✅ Tous les bots sont arrêtés proprement.");
}

// ============================================================================
// LOGIQUE PRINCIPALE DU BOT
// ============================================================================

async fn run_bot(bot_id: usize, mut stop_rx: watch::Receiver<bool>) {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap();

    let username = format!("user{}", bot_id);
    let password = format!("password{}", bot_id);

    // 1. Inscription (optionnelle, on ignore l'erreur si le compte existe déjà)
    let _ = client
        .post(format!("{}/register", GATEKEEPER_URL))
        .json(&serde_json::json!({ "username": username, "password": password }))
        .send()
        .await;

    // 2. Authentification
    let login_res = match client
        .post(format!("{}/login", GATEKEEPER_URL))
        .basic_auth(&username, Some(&password))
        .send()
        .await
    {
        Ok(res) if res.status().is_success() => res.text().await.unwrap_or_default(),
        _ => {
            eprintln!("Bot {} : Échec de connexion au Gatekeeper.", bot_id);
            return;
        }
    };

    let jwt_token = login_res;

    // Extraction du CustomId
    let parts: Vec<&str> = jwt_token.split('.').collect();
    let payload = URL_SAFE_NO_PAD.decode(parts[1]).unwrap();
    let claims: serde_json::Value = serde_json::from_slice(&payload).unwrap();
    let custom_id = claims["custom_id"].as_u64().unwrap() as u32;

    // 3. Demande du serveur (Broker)
    let server_res = match client
        .get(format!("{}/server", GATEKEEPER_URL))
        .bearer_auth(&jwt_token)
        .send()
        .await
    {
        Ok(res) if res.status().is_success() => res.text().await.unwrap_or_default(),
        _ => {
            eprintln!("Bot {} : Impossible d'obtenir l'adresse du serveur.", bot_id);
            return;
        }
    };

    let server_data: serde_json::Value = serde_json::from_str(&server_res).unwrap();
    let broker_addr = server_data["broker_addr"].as_str().unwrap();
    let session_token = server_data["session_token"].as_str().unwrap().to_string();

    let parts: Vec<&str> = broker_addr.split(':').collect();
    let mut ip = parts[0].to_string();
    let port: u16 = parts[1].parse().unwrap();

    // Redirection localhost
    if ip.starts_with("172.") || ip.starts_with("10.") {
        ip = "127.0.0.1".to_string();
    }

    // 4. Initialisation Réseau QUIC
    let backend = QuicBackend::new();
    let mut peer = GamePeer::new(backend);

    if peer.connect(&ip, port).is_err() {
        eprintln!("Bot {} : Échec de création du socket QUIC.", bot_id);
        return;
    }

    let mut connection = None;
    let mut bot_pos = (50000.0, 50000.0); // Position par défaut au centre

    // Simulation de l'état d'input (pour garder une direction quelques ticks)
    let mut current_input = 0u8;
    let mut input_duration = 0;

    let mut ticker = time::interval(Duration::from_millis(1000 / TICK_RATE_HZ));

    // 5. Boucle Principale du Bot
    loop {
        tokio::select! {
            _ = ticker.tick() => {
                // A. Lecture des événements réseau
                while let Ok(Some(event)) = peer.poll() {
                    match event {
                        GameNetworkEvent::Connected(conn) => {
                            connection = Some(conn.clone());
                            let _ = peer.create_stream(conn, GameStreamReliability::Reliable);
                        }
                        GameNetworkEvent::StreamCreated(conn, stream) => {
                            if stream.is_reliable() {
                                let handshake = BrokerHandshakeClient {
                                    jwt_token: session_token.as_bytes().to_vec(),
                                };
                                let _ = peer.send(&conn, &stream, handshake.to_bytes());
                                println!("Bot {} : En jeu !", bot_id);
                            }
                        }
                        GameNetworkEvent::Message { stream, data, .. } => {
                            if data.is_empty() { continue; }
                            
                            match data[0] {
                                Broadcast::TAG => {
                                    if let Some(pkt) = Broadcast::try_from_bytes(data) {
                                        if let Some(GameMessage::WorldSync(sync_data)) = GameMessage::decode(stream.stream_id, &pkt.payload) {
                                            // Mise à jour de la position du bot
                                            for player in sync_data.entities {
                                                if player.client_id.as_u32() == custom_id {
                                                    bot_pos = (player.pos.0, player.pos.1);
                                                    break;
                                                }
                                            }
                                        }
                                    }
                                }
                                RefuseClient::TAG => {
                                    eprintln!("Bot {} : Refusé par le serveur.", bot_id);
                                    return;
                                }
                                _ => {}
                            }
                        }
                        GameNetworkEvent::Disconnected(_) => {
                            println!("Bot {} : Déconnecté.", bot_id);
                            return;
                        }
                        _ => {}
                    }
                }

                // B. Envoi des Inputs si connecté
                if let Some(conn) = &connection {
                    current_input = generate_smart_input(&mut input_duration, current_input, bot_pos);

                    let sync_payload = PlayerInputPayload {
                        inputs: std::array::from_fn(|_| PlayerInput { input: current_input }),
                    };

                    let publish_packet = Publish {
                        topic_id: CustomId::from(custom_id),
                        payload: bitcode::encode(&sync_payload),
                    };

                    let stream = GameStream::new(
                        LogicalStream::Input as u16,
                        GameStreamReliability::Unreliable,
                    );

                    let _ = peer.send(conn, &stream, publish_packet.to_bytes());
                }
            }
            
            // Écoute du signal d'arrêt
            _ = stop_rx.changed() => {
                if *stop_rx.borrow() {
                    break; // Sort de la boucle proprement
                }
            }
        }
    }
}

// ============================================================================
// INTELLIGENCE DE DÉPLACEMENT DU BOT
// ============================================================================

fn generate_smart_input(duration: &mut u32, current_input: u8, pos: (f32, f32)) -> u8 {
    // 1. Vérification des bords (Priorité absolue pour rester dans la map)
    let mut forced_input = 0;
    let mut is_forced = false;

    if pos.0 < MAP_MIN { forced_input |= INPUT_RIGHT; is_forced = true; }
    if pos.0 > MAP_MAX { forced_input |= INPUT_LEFT; is_forced = true; }
    if pos.1 < MAP_MIN { forced_input |= INPUT_UP; is_forced = true; }
    if pos.1 > MAP_MAX { forced_input |= INPUT_DOWN; is_forced = true; }

    if is_forced {
        *duration = 30; // On maintient la fuite du bord pendant au moins 1 seconde (30 ticks)
        return forced_input;
    }

    // 2. Si on est en train de maintenir un mouvement valide, on le conserve !
    if *duration > 0 {
        *duration -= 1;
        return current_input; // <-- C'est ici que ça bloquait, maintenant on renvoie l'input en cours !
    }

    // 3. Le temps est écoulé, on choisit une nouvelle direction et une nouvelle durée
    let mut rng = rand::thread_rng();
    *duration = rng.gen_range(15..=60); // Garde la direction entre 0.5s et 2s

    let mut new_input = 0;
    match rng.gen_range(0..5) {
        0 => new_input |= INPUT_UP,
        1 => new_input |= INPUT_DOWN,
        2 => new_input |= INPUT_LEFT,
        3 => new_input |= INPUT_RIGHT,
        _ => {} // 20% de chance de faire une petite pause (Idle)
    }

    new_input
}