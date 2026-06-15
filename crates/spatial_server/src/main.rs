mod quad_tree;
mod network;
mod spatial_service;
mod visualizer;

use bytes::Bytes;
use std::time::{Duration, Instant};
use std::{env, io, thread};
use std::io::Write;
use mathtools::Vec2;
use std::net::ToSocketAddrs;
use std::sync::{Arc, RwLock};
use network::{InfrastructureEvent, InfrastructureNetwork, PeerType};
use spatial_service::SpatialService;

use mmo_math_tools::rect::Rect;
use custom_id::shard_id::ShardId;
use internal_communication_protocol::internal_models::{CustomServerPacket, ServerBinaryPacket, SpawnServer};

fn main() {
    println!("Hello, world! I'm the SpatialServer. And I would like to ask you : comment tu t'appèèèlles ?");
    let mut spatial_service = SpatialService::new(
        Rect {
            min: Vec2::new(0.0, 0.0),
            max: Vec2::new(100000.0, 100000.0)
        },
        1,
        1000.0,
        70,
        50,
        15.0,
        1.5,
    );

    let orchestrator_addr = env::var("ORCHESTRATOR_ADDR").unwrap_or_else(|_| "127.0.0.1".to_string());
    let orchestrator_port: u16 = env::var("ORCHESTRATOR_PORT").unwrap_or_else(|_| "4000".to_string()).parse().unwrap();

    let broker_addr = env::var("BROKER_ADDR").unwrap_or_else(|_| "127.0.0.1".to_string());
    let broker_port: u16 = env::var("BROKER_PORT").unwrap_or_else(|_| "5000".to_string()).parse().unwrap();

    // --- RÉSOLUTION DNS ORCHESTRATEUR ---
    let orch_full = format!("{}:{}", orchestrator_addr, orchestrator_port);
    let orch_resolved = orch_full.to_socket_addrs()
        .expect("❌ Erreur DNS Orchestrateur")
        .next()
        .expect("❌ Aucune IP trouvée pour l'Orchestrateur");

    let resolved_orch_ip = orch_resolved.ip().to_string();
    let resolved_orch_port = orch_resolved.port();

    // --- RÉSOLUTION DNS BROKER ---
    let broker_full = format!("{}:{}", broker_addr, broker_port);
    let broker_resolved = broker_full.to_socket_addrs()
        .expect("❌ Erreur DNS Broker")
        .next()
        .expect("❌ Aucune IP trouvée pour le Broker");

    let resolved_broker_ip = broker_resolved.ip().to_string();
    let resolved_broker_port = broker_resolved.port();

    println!(
        "Démarrage SpatialServer. Cibles résolues -> Orchestrateur: {}:{} | Broker: {}:{}",
        resolved_orch_ip, resolved_orch_port, resolved_broker_ip, resolved_broker_port
    );

    // On passe les vraies adresses IP numériques au réseau !
    let mut infra_net = InfrastructureNetwork::new(
        &resolved_orch_ip, resolved_orch_port,
        &resolved_broker_ip, resolved_broker_port
    );

    let target_tick_duration = Duration::from_secs_f64(1.0 / 60.0);

    let shared_state = Arc::new(RwLock::new(String::from("{}")));

    let viz_state = shared_state.clone();
    thread::spawn(move || {
        let server = tiny_http::Server::http("0.0.0.0:8080").unwrap();
        for request in server.incoming_requests() {
            let state = viz_state.read().unwrap();

            let response = tiny_http::Response::from_string(state.clone())
                .with_header(tiny_http::Header::from_bytes(&b"Access-Control-Allow-Origin"[..], &b"*"[..]).unwrap())
                .with_header(tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap());

            let _ = request.respond(response);
        }
    });

    // 🚩 Drapeau pour s'assurer qu'on ne demande la Shard Root qu'une seule fois
    let mut root_shard_requested = false;

    // ⏱️ Chronomètre de démarrage pour laisser le temps à l'Orchestrateur de spawn ses serveurs
    let startup_time = Instant::now();
    let orchestrator_warmup_delay = Duration::from_secs(10); // 3 secondes de délai (ajustable)

    loop {
        let frame_start = Instant::now();
        let events = infra_net.poll_events();

        let mut cmd: Vec<(PeerType, Bytes)> = Vec::new();

        for event in events {
            let event_result : Option<Vec<(PeerType,Bytes)>> =
                match event {
                    InfrastructureEvent::MessageReceived { source: PeerType::Broker, data } => {
                        handle_broker_data(data, &mut spatial_service)
                    }
                    InfrastructureEvent::MessageReceived { source: PeerType::Orchestrator, data } => {
                        // handle_orchestrator_data(data, &mut spatial_service) aucune donnée attendue de l'Orchestrateur
                        println!("Erreur : Message reçu de l'Orchestrateur : {:?}", data);
                        None
                    }
                    InfrastructureEvent::Disconnected { source } => {
                        eprintln!("Connexion perdue avec {:?}", source);
                        None
                    }
                };

            if let Some(mut new_packets) = event_result {
                cmd.append(&mut new_packets);
            }
        }

        // --- 🚀 INITIALISATION DE LA PREMIÈRE SHARD ---
        if !root_shard_requested && startup_time.elapsed() >= orchestrator_warmup_delay {
            let spawn_packet = SpawnServer {
                shard_id: ShardId::ROOT.into(), // Conversion automatique en u32
                pos_min: spatial_service.quad_tree.bounds.min,
                pos_max: spatial_service.quad_tree.bounds.max,
            };

            // On tente d'envoyer la commande.
            if infra_net.send_to_orchestrator(spawn_packet.to_bytes()).is_ok() {
                println!("🌱 [SpatialServer] Requête de spawn pour la Shard ROOT envoyée après {}s de chauffe !", orchestrator_warmup_delay.as_secs());
                root_shard_requested = true;
            }
        }
        // ----------------------------------------------

        if !cmd.is_empty() {
            for (peer_type, data) in cmd {
                match peer_type {
                    PeerType::Broker => {
                        let _ = infra_net.send_to_broker(data);
                    }
                    PeerType::Orchestrator => {
                        let _ = infra_net.send_to_orchestrator(data);
                    }
                }
            }
        }

        let json = visualizer::extract_viz_state(&spatial_service.quad_tree, spatial_service.margin);
        if let Ok(mut lock) = shared_state.write() {
            *lock = json;
        }

        let elapsed = frame_start.elapsed();
        if elapsed < target_tick_duration {
            std::thread::sleep(target_tick_duration - elapsed);
        }
        if let Err(e) = io::stdout().flush() {
            eprintln!("Erreur lors du flush stdout: {}", e);
        }
    }
}

// fn handle_orchestrator_data(raw_bytes: Bytes, spatial_service: &mut SpatialService) -> Option<Vec<(PeerType,Bytes)>> {
//     let cmd : Option<Vec<(PeerType,Bytes)>> =
//         match CustomServerPacket::try_from_bytes(raw_bytes) {
//             None => {
//                 eprintln!("Paquet binaire invalide ou Tag inconnu reçu de l'Orchestrateur.");
//                 None
//             }
//             _ => {
//                 eprintln!("Paquet reçu de l'Orchestrateur mais pas encore géré dans le SpatialService.");
//                 None
//             }
//         };
//     cmd
// }

fn handle_broker_data(raw_bytes: Bytes, spatial_service: &mut SpatialService) -> Option<Vec<(PeerType,Bytes)>> {
    let cmd : Option<Vec<(PeerType,Bytes)>> =
        match CustomServerPacket::try_from_bytes(raw_bytes) {
            Some(CustomServerPacket::ServerSpawned(update)) => {
                spatial_service.process_server_spawned(update)
            }
            Some(CustomServerPacket::PositionUpdate(update)) => {
                spatial_service.process_position_update(update)
            }
            Some(CustomServerPacket::HandoffAccept(update)) => {
                println!("handoff_accept");
                spatial_service.process_handoff_accept(update)
            }
            Some(CustomServerPacket::PlayerJoinUpdate(update)) => {
                spatial_service.process_player_join(update)
            }
            Some(CustomServerPacket::ClientLeft(update)) => {
                spatial_service.process_player_left(update)
            }
            Some(CustomServerPacket::ServerHeartBeat(update)) => {
                spatial_service.process_server_heartbeat(update)
            }
            None => {
                eprintln!("Paquet binaire invalide ou Tag inconnu reçu du Broker.");
                None
            }
            _ => {
                eprintln!("Paquet reçu du Broker mais pas encore géré dans le SpatialService.");
                None
            }
        };
    cmd
}