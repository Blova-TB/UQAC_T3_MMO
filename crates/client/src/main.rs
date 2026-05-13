use bevy::app::ScheduleRunnerPlugin;
use bevy::prelude::*;
use bitcode::{Decode, Encode};
use bytes::Bytes;
use std::time::Duration;

use shared::network::{GameConnection, GameNetworkEvent, GamePeer};
use shared::network::protocols::QuicBackend;

// --- Protocoles (Stricte copie de ceux du serveur) ---

#[derive(Encode, Decode, Debug)]
pub enum ClientPacket {
    Join { username: String },
}

#[derive(Encode, Decode, Debug)]
pub enum ServerPacket {
    Welcome { player_id: u64 },
    RejectedFull,
}

// --- État du client ---

#[derive(Resource)]
pub struct ClientState {
    pub peer: GamePeer,
    pub connection: Option<GameConnection>,
}

fn main() {
    App::new()
        .add_plugins(MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(
            Duration::from_secs_f64(1.0 / 60.0),
        )))
        .add_systems(Startup, setup_client)
        .add_systems(Update, handle_network)
        .run();
}

// --- Systèmes ---

fn setup_client(mut commands: Commands) {
    let backend = QuicBackend::new();
    let peer = GamePeer::new(backend);

    // Connexion au Serveur Dédié (Assurez-vous que le port correspond)
    peer.connect("127.0.0.1", 5000).expect("Échec de l'initialisation de la connexion");

    commands.insert_resource(ClientState {
        peer,
        connection: None,
    });

    println!("Client : Tentative de connexion au serveur 127.0.0.1:5000...");
}

fn handle_network(mut state: ResMut<ClientState>, mut exit: MessageWriter<AppExit>) {
    loop {
        match state.peer.poll() {
            Ok(Some(event)) => match event {
                GameNetworkEvent::Connected(conn) => {
                    println!("Client : Connecté au serveur QUIC. En attente de l'ouverture du flux...");
                    state.connection = Some(conn);
                }
                GameNetworkEvent::StreamCreated(conn, stream) => {
                    // Le serveur a créé le flux (cf. logique serveur). Le client l'utilise pour envoyer le JOIN.
                    println!("Client : Flux reçu. Envoi de la requête de connexion (Join)...");

                    let packet = ClientPacket::Join {
                        username: "TestPlayer_01".to_string(),
                    };

                    let encoded_data = bitcode::encode(&packet);
                    if let Err(e) = state.peer.send(&conn, &stream, Bytes::from(encoded_data)) {
                        eprintln!("Client : Échec de l'envoi du paquet: {:?}", e);
                    }
                }
                GameNetworkEvent::Message { data, .. } => {
                    // Décodage de la réponse du serveur
                    if let Ok(packet) = bitcode::decode::<ServerPacket>(&data) {
                        match packet {
                            ServerPacket::Welcome { player_id } => {
                                println!("Client : Succès ! Le serveur m'a assigné l'ID : {}", player_id);
                            }
                            ServerPacket::RejectedFull => {
                                println!("Client : Connexion refusée (Serveur plein).");
                                exit.write(AppExit::Success);
                            }
                        }
                    }
                }
                GameNetworkEvent::Disconnected(_) => {
                    println!("Client : Déconnecté par le serveur.");
                    exit.write(AppExit::Success);
                }
                GameNetworkEvent::Error { inner, .. } => {
                    eprintln!("Client : Erreur protocole : {:?}", inner);
                }
                _ => {}
            },
            Ok(None) => break,
            Err(e) => {
                eprintln!("Client : Erreur fatale de polling : {:?}", e);
                break;
            }
        }
    }
}