use crate::core::{ClientState, NetworkSnapshotEvent, SessionData, TargetServer};
use crate::states::AppState;
use bevy::prelude::*;

use client_communication_protocol::client_models::GameMessage;
use network_protocol::network::{GameNetworkEvent, GamePeer, GameStreamReliability};
use network_protocol::network::protocols::QuicBackend;
use internal_communication_protocol::internal_models::{Broadcast, BrokerHandshakeClient, RefuseClient, ServerBinaryPacket};

pub struct NetworkPlugin;

impl Plugin for NetworkPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(AppState::Connecting), init_game_connection)
            .add_systems(
                Update,
                handle_connection_handshake.run_if(in_state(AppState::Connecting)),
            )
            .add_systems(
                Update,
                handle_ingame_network.run_if(in_state(AppState::InGame)),
            );
    }
}

fn init_game_connection(mut commands: Commands, target: Res<TargetServer>) {
    let backend = QuicBackend::new();
    let peer = GamePeer::new(backend);

    println!(
        ">>> DEBUG: Tentative de connexion QUIC vers {}:{}",
        target.ip, target.port
    );

    if let Err(e) = peer.connect(&target.ip, target.port) {
        eprintln!(">>> ERREUR: Échec de la création du socket QUIC: {:?}", e);
    }

    commands.insert_resource(ClientState {
        peer,
        connection: None,
    });
}

fn handle_connection_handshake(
    mut state: ResMut<ClientState>,
    session: Res<SessionData>,
    mut next_state: ResMut<NextState<AppState>>,
    mut exit: MessageWriter<AppExit>,
) {
    while let Ok(Some(event)) = state.peer.poll() {
        match event {
            GameNetworkEvent::Connected(conn) => {
                state.connection = Some(conn.clone());
                let _ = state.peer.create_stream(conn, GameStreamReliability::Reliable);
            }
            GameNetworkEvent::StreamCreated(conn, stream) => {
                if stream.is_reliable() {
                    let handshake = BrokerHandshakeClient {
                        jwt_token: session.session_token.as_bytes().to_vec(),
                    };

                    if state.peer.send(&conn, &stream, handshake.to_bytes()).is_ok() {
                        next_state.set(AppState::InGame);
                    } else {
                        exit.write(AppExit::Error(std::num::NonZeroU8::new(1).unwrap()));
                    }
                }
            }
            GameNetworkEvent::Disconnected(_) => {
                exit.write(AppExit::Success);
            }
            _ => {}
        }
    }
}

fn handle_ingame_network(
    mut state: ResMut<ClientState>,
    mut ev_snapshot: MessageWriter<NetworkSnapshotEvent>,
    mut next_state: ResMut<NextState<AppState>>,
) {
    while let Ok(Some(event)) = state.peer.poll() {
        match event {
            GameNetworkEvent::Message { stream, data, .. } => {
                if data.is_empty() {
                    return;
                }
                let tag = data[0];

                match tag {
                    Broadcast::TAG => {
                        let Some(pkt) = Broadcast::try_from_bytes(data) else {
                            return;
                        };
                        if let Some(game_message) =
                            GameMessage::decode(stream.stream_id, &pkt.payload)
                        {
                            if let GameMessage::WorldSync(sync_data) = game_message {
                                println!("📡 Snapshot reçu du Broker : {} entités", sync_data.entities.len());
                                ev_snapshot.write(NetworkSnapshotEvent {
                                    players: sync_data.entities,
                                });
                            }
                        }
                    }
                    RefuseClient::TAG => {
                        eprintln!("❌ Le Broker a refusé la connexion du client.");
                        next_state.set(AppState::AuthMenu);
                    }
                    _ => {}
                }
            }
            GameNetworkEvent::Disconnected(_) => {
                println!("Client : Session fermée par le Broker.");
                next_state.set(AppState::AuthMenu);
            }
            GameNetworkEvent::Error { inner, .. } => {
                eprintln!("Client : Erreur réseau : {:?}", inner);
            }
            _ => {}
        }
    }
}