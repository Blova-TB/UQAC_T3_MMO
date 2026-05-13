use bevy::prelude::*;
use std::collections::HashMap;
use bytes::Bytes;
use bitcode::{Encode, Decode};

// Importation de votre librairie interne
use shared::network::{
    GameConnection, GameNetworkEvent, GamePeer, GameStream, GameStreamReliability,
};
use shared::network::protocols::QuicBackend;

use crate::player::{Player, PlayerBundle};

// --- Modèles de Données Réseau ---

#[derive(Encode, Decode)]
pub struct ServerSyncMessage {
    pub players: Vec<PlayerPositionData>,
}

#[derive(Encode, Decode)]
pub struct PlayerPositionData {
    pub entity_bits: u64,
    pub position: [f32; 2],
}

// Ajoutez un Buffer d'encodage global dans vos ressources système pour éviter
// d'allouer de la mémoire à chaque itération.
#[derive(Resource, Default)]
pub struct NetworkEncoder {
    pub buffer: bitcode::Buffer,
}

// --- Ressources ECS ---

#[derive(Resource)]
pub struct NetworkTransport {
    pub peer: GamePeer,
}

pub struct ClientData {
    pub entity: Entity,
    pub unreliable_stream: Option<GameStream>,
}

#[derive(Resource, Default)]
pub struct ServerClients {
    pub map: HashMap<GameConnection, ClientData>,
}

// --- Plugin ---

pub struct NetworkServerPlugin;

impl Plugin for NetworkServerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ServerClients>()
            .init_resource::<NetworkEncoder>()
            .add_systems(Startup, setup_network)
            .add_systems(
                Update,
                (poll_network_events, broadcast_positions).chain(),
            );
    }
}

// --- Systèmes ---

fn setup_network(mut commands: Commands) {
    let backend = QuicBackend::new();
    let peer = GamePeer::new(backend);

    // Arrêt strict du serveur si le port est indisponible
    peer.listen("0.0.0.0", 5000).unwrap_or_else(|e| {
        panic!("Critical failure: Cannot bind QUIC socket on port 5000. Error: {:?}", e);
    });

    commands.insert_resource(NetworkTransport { peer });
    println!("QUIC Server initialized and listening on 0.0.0.0:5000");
}

fn poll_network_events(
    mut commands: Commands,
    mut transport: ResMut<NetworkTransport>,
    mut clients: ResMut<ServerClients>,
) {
    // Dépilement complet de la file d'événements à chaque frame
    loop {
        match transport.peer.poll() {
            Ok(Some(event)) => match event {
                GameNetworkEvent::Connected(conn) => {
                    println!("Client connected: {:?}", conn.connection_id);

                    // Instanciation de l'entité joueur avec des valeurs par défaut
                    let entity = commands
                        .spawn(PlayerBundle::new(Vec2::ZERO, 100, 300.0, 15.0))
                        .id();

                    clients.map.insert(
                        conn,
                        ClientData {
                            entity,
                            unreliable_stream: None,
                        },
                    );

                    // Initialisation immédiate d'un flux Unreliable pour la physique
                    if let Err(e) = transport.peer.create_stream(conn, GameStreamReliability::Unreliable) {
                        eprintln!("Network Error: Failed to create Unreliable stream for {:?}: {:?}", conn.connection_id, e);
                    }
                }
                GameNetworkEvent::Disconnected(conn) => {
                    println!("Client disconnected: {:?}", conn.connection_id);
                    if let Some(client_data) = clients.map.remove(&conn) {
                        commands.entity(client_data.entity).despawn();
                    }
                }
                GameNetworkEvent::StreamCreated(conn, stream) => {
                    // Assignation du flux physique au client correspondant
                    if let Some(client_data) = clients.map.get_mut(&conn) {
                        if !stream.is_reliable() {
                            client_data.unreliable_stream = Some(stream);
                        }
                    }
                }
                GameNetworkEvent::Message { connection, stream, data } => {
                    // Placeholder: Traitement des inputs client (à implémenter)
                    let _ = (connection, stream, data);
                }
                GameNetworkEvent::Error { connection, inner } => {
                    eprintln!("Network protocol error for {:?}: {:?}", connection.connection_id, inner);
                }
                GameNetworkEvent::StreamClosed(_conn, _stream) => {
                    // Géré implicitement lors de la déconnexion globale pour le moment
                }
            },
            Ok(None) => break, // File vide
            Err(e) => {
                eprintln!("Critical hardware/socket error during polling: {:?}", e);
                break;
            }
        }
    }
}

fn broadcast_positions(
    transport: Res<NetworkTransport>,
    clients: Res<ServerClients>,
    mut encoder: ResMut<NetworkEncoder>, // Injection du buffer réutilisable
    query: Query<(Entity, &Transform), With<Player>>,
) {
    if clients.map.is_empty() {
        return;
    }

    // 1. Agrégation des données de la frame
    let mut sync_msg = ServerSyncMessage {
        players: Vec::with_capacity(query.iter().len()),
    };

    for (entity, transform) in query.iter() {
        sync_msg.players.push(PlayerPositionData {
            entity_bits: entity.to_bits(),
            position: transform.translation.truncate().to_array(), // Conversion en [f32; 2]
        });
    }

    // 2. Sérialisation stricte
    // Encodage avec le buffer réutilisable
    let encoded_data = encoder.buffer.encode(&sync_msg);
    let serialized_bytes = Bytes::copy_from_slice(encoded_data);

    // 3. Diffusion (Broadcasting) à tous les clients disposant d'un flux valide
    for (conn, client_data) in clients.map.iter() {
        if let Some(stream) = &client_data.unreliable_stream {
            if let Err(e) = transport.peer.send(conn, stream, serialized_bytes.clone()) {
                eprintln!("Transport fault: {:?}", e);
            }
        }
    }
}