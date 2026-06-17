use crate::config::ServerConfig;
use crate::core::AssignedShard;
use crate::events::{BrokerCommand, BrokerEvent};
use crate::player::{Ghost, Player};
use crate::states::ServerState;
use bevy::prelude::*;
use mathtools::Vec2 as MathVec2;

use custom_id::custom_id::CustomId;
use client_communication_protocol::client_models::{GameMessage, LogicalStream};
use custom_id::chunk_id::ChunkId;
use internal_communication_protocol::internal_models::*;
use network_protocol::network::{GameConnection, GameNetworkEvent, GamePeer, GameStream, GameStreamReliability};
use network_protocol::network::protocols::QuicBackend;


pub struct BrokerPlugin;

impl Plugin for BrokerPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(ServerState::Active), connect_to_broker)
            .add_systems(
                Update,
                poll_broker
                    .chain()
                    .run_if(in_state(ServerState::Active)),
            )
            .add_systems(
                PostUpdate,
                (send_broker_commands, send_broker_heartbeat)
                    .chain()
                    .run_if(in_state(ServerState::Active)),
            );
    }
}

#[derive(Resource)]
pub struct BrokerConnection {
    pub peer: GamePeer,
    pub connection: Option<GameConnection>,
    pub topic: u32,
    pub reliable_stream: Option<GameStream>,
}

fn connect_to_broker(
    mut commands: Commands,
    config: Res<ServerConfig>,
    assigned_shard: Res<AssignedShard>,
) {
    let peer = GamePeer::new(QuicBackend::new());
    let addr: std::net::SocketAddr = config.broker_addr.parse().expect("❌ BROKER_ADDR invalide");

    peer.connect(&addr.ip().to_string(), addr.port()).unwrap_or_else(|e| {
        panic!("Échec de connexion au Broker sur {}: {:?}", config.broker_addr, e);
    });

    let shard_id_u32 = assigned_shard.0.as_u32();

    commands.insert_resource(BrokerConnection {
        peer,
        connection: None,
        topic: shard_id_u32,
        reliable_stream: None,
    });

    info!("🔗 Shard (ID: {}) tente de se connecter au Broker.", shard_id_u32);
}

fn poll_broker(
    mut broker: ResMut<BrokerConnection>,
    mut ev_broker: MessageWriter<BrokerEvent>,
) {
    loop {
        match broker.peer.poll() {
            Ok(Some(event)) => match event {
                GameNetworkEvent::Connected(conn) => {
                    info!("✅ Connecté au Broker avec succès !");
                    broker.connection = Some(conn);
                    let _ = broker.peer.create_stream(conn, GameStreamReliability::Reliable);
                }
                GameNetworkEvent::StreamCreated(conn, stream) => {
                    if stream.is_reliable() {
                        broker.reliable_stream = Some(stream.clone());
                        let handshake = BrokerHandshakeShard {
                            shard_id: CustomId::from(broker.topic),
                        };
                        let _ = broker.peer.send(&conn, &stream, handshake.to_bytes());
                        info!("🌍 Shard Handshake envoyé !");
                    }
                }
                GameNetworkEvent::Message { stream, data, .. } => {
                    if data.is_empty() {
                        continue;
                    }

                    match data[0] {
                        SpawnPlayerShard::TAG => {
                            if let Some(pkt) = SpawnPlayerShard::try_from_bytes(data) {
                                ev_broker.write(BrokerEvent::SpawnPlayer {
                                    client_id: pkt.client_id,
                                    pos: Vec2::new(pkt.pos.x, pkt.pos.y),
                                });
                            }
                        }
                        ClientLeft::TAG => {
                            if let Some(pkt) = ClientLeft::try_from_bytes(data) {
                                ev_broker.write(BrokerEvent::PlayerLeft {
                                    client_id: pkt.client_id,
                                });
                            }
                        }
                        BroadcastClient::TAG => {
                            if let Some(pkt) = BroadcastClient::try_from_bytes(data) {
                                if let Some(game_message) =
                                    GameMessage::decode(stream.stream_id, &pkt.payload)
                                {
                                    match game_message {
                                        GameMessage::Input(input_payload) => {
                                            ev_broker.write(BrokerEvent::PlayerInput {
                                                client_id: pkt.client_id,
                                                payload: input_payload,
                                            });
                                        }
                                        _ => {}
                                    }
                                }
                            }
                        }
                        ShutdownServerOnEmpty::TAG => {
                            if let Some(_) = ShutdownServerOnEmpty::try_from_bytes(data) {
                                ev_broker.write(BrokerEvent::ShutdownRequested);
                                info!("🛑 Signal de Shutdown reçu du Broker ! Le serveur se fermera une fois vide.");
                            }
                        }
                        HandoffRequest::TAG => {
                            if let Some(pkt) = HandoffRequest::try_from_bytes(data) {
                                ev_broker.write(BrokerEvent::SpawnGhostPlayer {
                                    shard_id: pkt.shard_id,
                                    client_id: pkt.entity_id,
                                });
                            }
                        }
                        TakeAuthority::TAG => {
                            if let Some(pkt) = TakeAuthority::try_from_bytes(data) {
                                ev_broker.write(BrokerEvent::TakeAuthority {
                                    client_id: pkt.entity_id,
                                    new_pos: Vec2::new(pkt.pos.x, pkt.pos.y),
                                });
                            }
                        }
                        DropAuthority::TAG => {
                            if let Some(pkt) = DropAuthority::try_from_bytes(data) {
                                ev_broker.write(BrokerEvent::DropAuthority {
                                    client_id: pkt.entity_id,
                                });
                            }
                        }
                        HandoffDrop::TAG => {
                            if let Some(pkt) = DropAuthority::try_from_bytes(data) {
                                ev_broker.write(BrokerEvent::HandoffDrop {
                                    client_id: pkt.entity_id,
                                });
                            }
                        }
                        _ => warn!("Tag non reconnu : 0x{:02X}", data[0]),
                    }
                }
                GameNetworkEvent::Disconnected(_) => {
                    warn!("❌ Déconnecté du Broker !");
                    broker.connection = None;
                    broker.reliable_stream = None;
                }
                _ => {}
            },
            Ok(None) => break,
            Err(e) => {
                error!("Erreur de polling Broker : {:?}", e);
                break;
            }
        }
    }
}

fn send_broker_commands(
    broker: Res<BrokerConnection>,
    mut ev_commands: MessageReader<BrokerCommand>,
) {
    let Some(conn) = &broker.connection else { return };

    for command in ev_commands.read() {
        match command {
            BrokerCommand::SendWorldSync(sync_payload) => {
                let publish_packet = Publish {
                    topic_id: CustomId::from(broker.topic),
                    payload: bitcode::encode(sync_payload),
                };
                let stream = GameStream::new(
                    LogicalStream::WorldSync as u16,
                    GameStreamReliability::Unreliable,
                );

                let _ = broker.peer.send(conn, &stream, publish_packet.to_bytes());
            }
            BrokerCommand::SendPositionUpdate { client_id, pos } => {
                let packet = PositionUpdate {
                    client_id: *client_id,
                    pos: MathVec2::new(pos.x, pos.y),
                };
                let stream = GameStream::new(0, GameStreamReliability::Unreliable);

                let _ = broker.peer.send(conn, &stream, packet.to_bytes());
            }
            BrokerCommand::SendHandoffAccept { shard_id, entity_id } => {
                let Some(stream) = broker.reliable_stream.as_ref() else {
                    warn!("Impossible d'envoyer HandoffAccept: stream fiable non initialisé");
                    continue;
                };

                let accept = HandoffAccept {
                    shard_id: *shard_id,
                    entity_id: *entity_id,
                };

                let _ = broker.peer.send(conn, stream, accept.to_bytes());
            }
            BrokerCommand::SendAoiModeChange { client_id, pos, mode } => {
                let Some(stream) = broker.reliable_stream.as_ref() else {
                    warn!("Impossible d'envoyer AoiModeChange: stream fiable non initialisé");
                    continue;
                };
                
                let packet = AoiModeChange {
                    client_id: *client_id,
                    chunk_id: CustomId::from(
                        ChunkId::from_position(mathtools::vector::Vec2::new(pos.x, pos.y)).unwrap_or_else(|e| {
                            panic!("Erreur de conversion de position en chunk_id pour AOI Mode Change: {}", e);
                        })
                    ),
                    new_mode: mode.to_u8(),
                };

                let _ = broker.peer.send(conn, stream, packet.to_bytes());
            }

            BrokerCommand::AoiPosUpdate { client_id, pos } => {
                let packet = AoiPosUpdate {
                    client_id: *client_id,
                    chunk_id: CustomId::from(
                        ChunkId::from_position(mathtools::vector::Vec2::new(pos.x, pos.y)).unwrap_or_else(|e| {
                            panic!("Erreur de conversion de position en chunk_id pour AOI Mode Change: {}", e);
                        })
                    ),
                };
                let stream = GameStream::new(0, GameStreamReliability::Unreliable);

                let _ = broker.peer.send(conn, &stream, packet.to_bytes());
            }
        }
    }
}

fn send_broker_heartbeat(
    time: Res<Time>,
    mut heartbeat_timer: Local<Timer>,
    broker: Res<BrokerConnection>,
    config: Res<ServerConfig>,
    player_query: Query<(), (With<Player>, Without<Ghost>)>,
) {
    if heartbeat_timer.duration() == std::time::Duration::ZERO {
        *heartbeat_timer = Timer::from_seconds(1.0, TimerMode::Repeating);
    }

    let Some(conn) = &broker.connection else { return };

    if heartbeat_timer.tick(time.delta()).just_finished() {
        let player_count = player_query.iter().count();
        let occupancy_percent = (player_count.saturating_mul(100) / config.max_players.max(1)).min(100) as u8;

        let packet = ServerHeartBeat {
            shard_id: CustomId::from(broker.topic),
            occupancy: occupancy_percent,
        };

        let stream = GameStream::new(0, GameStreamReliability::Unreliable);
        let _ = broker.peer.send(conn, &stream, packet.to_bytes());
    }
}