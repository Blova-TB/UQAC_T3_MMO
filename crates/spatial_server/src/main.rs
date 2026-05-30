mod quad_tree;
mod shard_id;
mod network;
mod spatial_service;

use bytes::Bytes;
use shared::models::SpatialServerPacket;
use std::time::{Duration, Instant};
use std::{env, io};
use std::io::Write;
use mathtools::Vec2;
use quad_tree::Rect;
use network::{InfrastructureEvent, InfrastructureNetwork, PeerType};
use spatial_service::SpatialService;

fn main() {
    println!("Hello, world! I'm the SpatialServer. And I would like to ask you : comment tu t'appèèèlles ?");
    let mut spatial_service = SpatialService::new(
        Rect {
            min: Vec2::new(0.0, 0.0),
            max: Vec2::new(1000.0, 1000.0)
        },
        6,
        10.0,
        0.8,
        0.5,
        5.0 // pas encore utilisé pour le moment
    );

    // Extraction stricte des variables d'environnement
    let orchestrator_addr = env::var("ORCHESTRATOR_ADDR")
        .unwrap_or_else(|_| "127.0.0.1".to_string());

    let orchestrator_port: u16 = env::var("ORCHESTRATOR_PORT")
        .unwrap_or_else(|_| "5000".to_string())
        .parse()
        .expect("ORCHESTRATOR_PORT doit être un nombre valide (u16)");

    let broker_addr = env::var("BROKER_ADDR")
        .unwrap_or_else(|_| "127.0.0.1".to_string());

    let broker_port: u16 = env::var("BROKER_PORT")
        .unwrap_or_else(|_| "6000".to_string())
        .parse()
        .expect("BROKER_PORT doit être un nombre valide (u16)");

    println!(
        "Démarrage SpatialServer. Cible Orchestrateur: {}:{} | Cible Broker: {}:{}",
        orchestrator_addr, orchestrator_port, broker_addr, broker_port
    );

    let mut infra_net = InfrastructureNetwork::new(
        &orchestrator_addr, orchestrator_port,
        &broker_addr, broker_port
    );

    let target_tick_duration = Duration::from_secs_f64(1.0 / 60.0);

    loop {
        let frame_start = Instant::now();
        let events = infra_net.poll_events();

        let mut cmd: Vec<(PeerType, Bytes)> = Vec::new();

        for event in events {
            let event_result : Option<Vec<(PeerType,Bytes)>> =
                match event {
                    InfrastructureEvent::MessageReceived { source: PeerType::Broker, data } => {
                        handle_broker_data(data, &mut spatial_service, &mut infra_net)
                    }
                    InfrastructureEvent::MessageReceived { source: PeerType::Orchestrator, data } => {
                        handle_orchestrator_data(data, &mut spatial_service)
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
        }else {
            println!("Error processing packet, no response generated.");
        }

        let elapsed = frame_start.elapsed();
        if elapsed < target_tick_duration {
            std::thread::sleep(target_tick_duration - elapsed);
        }
        print!("\rTick processed in {:?} ms", elapsed.as_millis());
        if let Err(e) = io::stdout().flush() {
            eprintln!("Erreur lors du flush stdout: {}", e);
        }
    }
}

fn handle_orchestrator_data(p0: Bytes, p1: &mut SpatialService) -> Option<Vec<(PeerType,Bytes)>> {
    // on recoit rien ici il me semble
    print!("Error : Received data from Orchestrator: {:?} bytes", p0.len());
    None
}

fn handle_broker_data(raw_bytes: Bytes, spatial_service: &mut SpatialService, infra_net: &mut InfrastructureNetwork) -> Option<Vec<(PeerType,Bytes)>> {

    let cmd : Option<Vec<(PeerType,Bytes)>> =
        match SpatialServerPacket::try_from_bytes(raw_bytes) {
            Some(SpatialServerPacket::Position(update)) => {
                spatial_service.process_update(update)
            }

            Some(SpatialServerPacket::ServerHandShake(update)) => {
                spatial_service.process_server_handshake(update)
            }

            None => {
                eprintln!("Paquet binaire invalide ou Tag inconnu reçu.");
                None
            }

            _ => {
                eprintln!("Paquet reçu mais pas encore géré dans le SpatialService.");
                None
            }
        };
    cmd
}