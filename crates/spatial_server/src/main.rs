mod quad_tree;
mod shard_id;
mod network;
mod spatial_service;

use bytes::Bytes;
use quad_tree::Rect;
use shared::models::SpatialServerPacket;
use std::time::{Duration, Instant};
use mathtools::Vec2;
use crate::network::{InfrastructureEvent, InfrastructureNetwork, PeerType};
use crate::spatial_service::SpatialService;

fn main() {
    println!("Hello, world!");
    let mut spatial_service = SpatialService::new(
        Rect {
            min: Vec2::new(0.0,0.0) ,
            max: Vec2::new(1000.0,1000.0)
        },
        6,
        10.0
    );

    // Initialisation du module réseau -> a modifier avec docker
    let mut infra_net = InfrastructureNetwork::new(
        "127.0.0.1", 5000, // Orchestrateur
        "127.0.0.1", 6000  // Broker
    );

    let target_tick_duration = Duration::from_secs_f64(1.0 / 60.0);

    loop {
        let frame_start = Instant::now();

        let events = infra_net.poll_events();

        for event in events {
            match event {
                InfrastructureEvent::MessageReceived { source: PeerType::Broker, data } => {
                    handle_broker_data(data, &mut spatial_service, &mut infra_net);
                }
                InfrastructureEvent::MessageReceived { source: PeerType::Orchestrator, data } => {
                    handle_orchestrator_data(data, &mut spatial_service);
                }
                InfrastructureEvent::Disconnected { source } => {
                    eprintln!("Connexion perdue avec {:?}", source);
                }
            }
        }

        let elapsed = frame_start.elapsed();
        if elapsed < target_tick_duration {
            std::thread::sleep(target_tick_duration - elapsed);
        }
    }
}

fn handle_orchestrator_data(p0: Bytes, p1: &mut SpatialService) {
    // on recoit rien ici il me semble
    print!("Error : Received data from Orchestrator: {:?} bytes", p0.len());
}

fn handle_broker_data(raw_bytes: Bytes, spatial_service: &mut SpatialService, infra_net: &mut InfrastructureNetwork) {

    let cmd : Option<Vec<(PeerType,Bytes)>> =
        match SpatialServerPacket::try_from_bytes(raw_bytes) {
            Some(SpatialServerPacket::Position(update)) => {
                spatial_service.process_update(update)
            }

            Some(SpatialServerPacket::Subdivide(update)) => {
                spatial_service.process_subdivide(update)
            }

            Some(SpatialServerPacket::PlayerJoin(update)) => {
                spatial_service.process_player_join(update)
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

    if let Some(packets) = cmd {
        for (peer_type, data) in packets {
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
}