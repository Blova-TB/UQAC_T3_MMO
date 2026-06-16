use std::{env, io};
use std::io::Write;
use std::net::ToSocketAddrs;
use std::time::{Duration, Instant};
use bytes::Bytes;
use aoi_model::aoi_model::AoiMode;
use custom_id::chunk_id::ChunkId;
use internal_communication_protocol::internal_models::{CustomServerPacket, ServerBinaryPacket};
use crate::aoi_service::AoiService;
use crate::network::{InfrastructureEvent, InfrastructureNetwork};

mod network;
mod aoi_service;

fn main() {
    println!("Hello, world! I'm the aoi_server.");

    let mut aoi_service = aoi_service::AoiService::new();


    let broker_addr = env::var("BROKER_ADDR").unwrap_or_else(|_| "127.0.0.1".to_string());
    let broker_port: u16 = env::var("BROKER_PORT").unwrap_or_else(|_| "5000".to_string()).parse().unwrap();

    // --- RÉSOLUTION DNS BROKER ---
    let broker_full = format!("{}:{}", broker_addr, broker_port);
    let broker_resolved = broker_full.to_socket_addrs()
        .expect("❌ Erreur DNS Broker")
        .next()
        .expect("❌ Aucune IP trouvée pour le Broker");

    let resolved_broker_ip = broker_resolved.ip().to_string();
    let resolved_broker_port = broker_resolved.port();

    println!(
        "Démarrage aoi_server. Cibles résolues -> Broker: {}:{}",
        resolved_broker_ip, resolved_broker_port
    );

    let mut infra_net = InfrastructureNetwork::new(
        &resolved_broker_ip, resolved_broker_port
    );

    let target_tick_duration = Duration::from_secs_f64(1.0 / 60.0);


    loop{
        let frame_start = Instant::now();
        let events = infra_net.poll_events();

        // traiteent des events
        for event in events {
            match event {
                InfrastructureEvent::MessageReceived { data } => {
                    let _ = handle_broker_data(data, &mut aoi_service);
                },
                InfrastructureEvent::Disconnected {} => {
                    println!("Déconnecté du broker !");
                }
            }
        }


        // on envoye les sub et les unsub
        for sub in aoi_service.frame_result.sub.drain(..) {
            infra_net.send_to_broker(sub.to_bytes()).unwrap_or(eprintln!("Erreur lors de l'envoi d'un sub au broker"));
        }
        for unsub in aoi_service.frame_result.unsub.drain(..) {
            infra_net.send_to_broker(unsub.to_bytes()).unwrap_or(eprintln!("Erreur lors de l'envoi d'un unsub au broker"));
        }

        //todo: full reliable car aucune gestion de perte de paquets ...

        let elapsed = frame_start.elapsed();
        if elapsed < target_tick_duration {
            std::thread::sleep(target_tick_duration - elapsed);
        }
        if let Err(e) = io::stdout().flush() {
            eprintln!("Erreur lors du flush stdout: {}", e);
        }
    }
}


fn handle_broker_data(data: Bytes, aoi_service: &mut AoiService) -> Result<(), String>{
    match CustomServerPacket::try_from_bytes(data) {
        Some(CustomServerPacket::AoiPosUpdate(update)) => {

            let chunk_id : ChunkId = update.chunk_id.try_into().map_err(|e| format!("ID de chunk invalide: {}", e))?;

            aoi_service.process_player_move(
                update.client_id.try_into().map_err(|e| format!("ID de client invalide: {}", e))?,
                chunk_id.to_chunk_coords(),
            )
        }
        Some(CustomServerPacket::AoiModeChange(update)) => {

            let chunk_id : ChunkId = update.chunk_id.try_into().map_err(|e| format!("ID de chunk invalide: {}", e))?;

            aoi_service.process_player_move_and_aoi_change(
                update.client_id.try_into().map_err(|e| format!("ID de client invalide: {}", e))?,
                chunk_id.to_chunk_coords(),
                AoiMode::from_u8(update.new_mode).ok_or("Mode AOI invalide")?,
            )
        }
        Some(CustomServerPacket::ClientLeft(update)) => {
            aoi_service.remove_player(
                update.client_id.try_into().map_err(|e| format!("ID de client invalide: {}", e))?
            )
        }
        _ => {
            eprintln!("Paquet binaire invalide ou Tag inconnu reçu du Broker.");
            Ok(())
        }
    }
}
