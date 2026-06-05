use ahash::AHashMap;
use bytes::Bytes;
use tracing::{error, info, warn};
use jsonwebtoken::{decode, DecodingKey, Validation};
use serde::{Deserialize, Serialize};

mod routing;
use routing::OptimizedRoutingTable;

use shared::models::*;
use shared::network::protocols::QuicBackend;
use shared::network::{GameConnection, GameNetworkEvent, GamePeer, GameStream, GameStreamReliability};

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,
    pub custom_id: u32,
    pub pos_x: f32,
    pub pos_y: f32,
    pub exp: usize,
}

struct BrokerState {
    conn_to_client: AHashMap<GameConnection, u32>,
    client_to_conn: AHashMap<u32, GameConnection>,
    client_streams: AHashMap<u32, GameStream>,
    routing_table: OptimizedRoutingTable,

    spatial_server_conn: Option<GameConnection>,
    spatial_server_stream: Option<GameStream>,

    shard_conns: AHashMap<u32, GameConnection>,
    shard_streams: AHashMap<u32, GameStream>,
}

impl Default for BrokerState {
    fn default() -> Self {
        Self {
            conn_to_client: AHashMap::new(),
            client_to_conn: AHashMap::new(),
            client_streams: AHashMap::new(),
            routing_table: OptimizedRoutingTable::default(),
            spatial_server_conn: None,
            spatial_server_stream: None,
            shard_conns: AHashMap::new(),
            shard_streams: AHashMap::new(),
        }
    }
}

fn main() {
    tracing_subscriber::fmt::init();
    info!("🚀 Démarrage du Broker PubSub Optimisé...");

    let mut state = BrokerState::default();
    let mut peer = GamePeer::new(QuicBackend::new());

    if let Err(e) = peer.listen("0.0.0.0", 5000) {
        error!("❌ Impossible de bind le port 5000: {:?}", e);
        return;
    }

    loop {
        match peer.poll() {
            Ok(Some(event)) => handle_network_event(&mut state, event, &mut peer),
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(1)),
            Err(e) => error!("❌ Erreur réseau critique: {:?}", e),
        }
    }
}

fn handle_network_event(state: &mut BrokerState, event: GameNetworkEvent, peer: &mut GamePeer) {
    match event {
        GameNetworkEvent::Disconnected(conn) => {
            if let Some(client_id) = state.conn_to_client.remove(&conn) {
                state.client_to_conn.remove(&client_id);
                state.client_streams.remove(&client_id);
                state.routing_table.unsubscribe_all(client_id);
                info!("👤 Client {} déconnecté et nettoyé.", client_id);
            }
            state.shard_conns.retain(|_, v| v != &conn);
            if Some(conn) == state.spatial_server_conn { state.spatial_server_conn = None; }
        }

        GameNetworkEvent::Message { connection, stream, data } => {
            if data.is_empty() { return; }
            let tag = data[0];

            match tag {
                BrokerHandshakeClient::TAG => {
                    let Some(pkt) = BrokerHandshakeClient::try_from_bytes(data) else { return; };
                    let token_str = String::from_utf8_lossy(&pkt.jwt_token);
                    let secret = std::env::var("JWT_SECRET").unwrap_or_else(|_| "secret_de_dev".to_string());

                    if let Ok(token_data) = decode::<Claims>(token_str.as_ref(), &DecodingKey::from_secret(secret.as_bytes()), &Validation::default()) {
                        let client_id = token_data.claims.custom_id;
                        state.conn_to_client.insert(connection.clone(), client_id);
                        state.client_to_conn.insert(client_id, connection);
                        state.client_streams.insert(client_id, stream.clone());

                        if let (Some(s_conn), Some(s_stream)) = (&state.spatial_server_conn, &state.spatial_server_stream) {
                            let _ = peer.send(s_conn, s_stream, PlayerJoinUpdate {
                                client_id: CustomId::from(client_id),
                                pos: Vec2::new(token_data.claims.pos_x, token_data.claims.pos_y),
                            }.to_bytes());
                        }
                        println!("PlayerJoinUpdate send with client id {} at position : x={}, y={}", client_id, token_data.claims.pos_x, token_data.claims.pos_y);
                    }
                }

                BrokerHandshakeShard::TAG => {
                    let Some(pkt) = BrokerHandshakeShard::try_from_bytes(data) else { return; };
                    let id = pkt.shard_id.into();
                    state.shard_conns.insert(id, connection);
                    state.shard_streams.insert(id, stream);

                    if let (Some(s_conn),Some(s_stream)) = (&state.spatial_server_conn, &state.spatial_server_stream) {
                        let _ = peer.send(s_conn, s_stream, ServerSpawned {
                            shard_id: CustomId::from(id)
                        }.to_bytes());
                    } else {
                        warn!("⚠️ PositionUpdate ignoré : Spatial Server non connecté.");
                    }
                }

                BrokerHandshakeSpatial::TAG => {
                    state.spatial_server_conn = Some(connection);
                    state.spatial_server_stream = Some(stream);
                }

                Subscribe::TAG => {
                    let Some(pkt) = Subscribe::try_from_bytes(data) else { return; };
                    state.routing_table.subscribe(pkt.custom_id.into(), pkt.topic_id);
                }

                Unsubscribe::TAG => {
                    let Some(pkt) = Unsubscribe::try_from_bytes(data) else { return; };
                    state.routing_table.unsubscribe(pkt.custom_id.into(), pkt.topic_id);
                }

                Publish::TAG => {
                    let Some(pkt) = Publish::try_from_bytes(data) else { return; };
                    let broadcast = Broadcast { payload: pkt.payload };
                    let final_msg = broadcast.to_bytes();
                    let topic_u32: u32 = pkt.topic_id.into();

                    if let Some(subscribers) = state.routing_table.get_subscribers(&topic_u32) {
                        for client_id in subscribers {
                            if let Some(client_conn) = state.client_to_conn.get(client_id) {
                                let _ = peer.send(client_conn, &stream, final_msg.clone());
                            }
                        }
                    }
                }

                ClientInput::TAG => {
                    let Some(pkt) = ClientInput::try_from_bytes(data.clone()) else { return; };
                    let topic_id = pkt.client_id.into();
                    if let Some(subs) = state.routing_table.get_subscribers(&topic_id) {
                        for &sub_id in subs {
                            if let Some(conn) = state.shard_conns.get(&sub_id) {
                                let _ = peer.send(conn, state.shard_streams.get(&sub_id).unwrap(), data.clone());
                            }
                        }
                    }
                }

                SpawnPlayerShard::TAG => {
                    let Some(pkt) = SpawnPlayerShard::try_from_bytes(data.clone()) else { return; };
                    let shard_id = pkt.shard_id.into();
                    state.routing_table.subscribe(pkt.client_id.into(), shard_id);
                    if let Some(conn) = state.shard_conns.get(&shard_id) {
                        let _ = peer.send(conn, state.shard_streams.get(&shard_id).unwrap(), data);
                    }
                }

                ServerHeartBeat::TAG => {
                    if let Some(s_conn) = &state.spatial_server_conn {
                        let unreliable_stream = GameStream::new(
                            shared::constants::STREAM_PHYSICS,
                            GameStreamReliability::Unreliable
                        );
                        let _ = peer.send(s_conn, &unreliable_stream, data);
                    } else {
                        warn!("⚠️ ServerHeartBeat ignoré : Spatial Server non connecté.");
                    }
                }

                PositionUpdate::TAG => {
                    if let Some(s_conn) = &state.spatial_server_conn {
                        let unreliable_stream = GameStream::new(
                            shared::constants::STREAM_PHYSICS,
                            GameStreamReliability::Unreliable
                        );
                        let _ = peer.send(s_conn, &unreliable_stream, data);
                    } else {
                        warn!("⚠️ PositionUpdate ignoré : Spatial Server non connecté.");
                    }
                }

                RefuseClient::TAG => {
                    let Some(pkt) = RefuseClient::try_from_bytes(data.clone()) else {
                        warn!("⚠️ Impossible de décoder le paquet RefuseClient");
                        return;
                    };

                    let client_id: u32 = pkt.client_id.into();

                    if let (Some(client_conn), Some(client_stream)) = (
                        state.client_to_conn.get(&client_id).cloned(),
                        state.client_streams.get(&client_id).cloned()
                    ) {
                        let _ = peer.send(&client_conn, &client_stream, data);

                        state.client_to_conn.remove(&client_id);
                        state.client_streams.remove(&client_id);
                        state.conn_to_client.remove(&client_conn);
                        state.routing_table.unsubscribe_all(client_id);

                        peer.disconnect(&client_conn).expect("échec fermeture connexion client");

                        info!("🚫 Client {} refusé : paquet transmis en reliable, état nettoyé.", client_id);
                    } else {
                        warn!("⚠️ Tentative de refus pour le client {}, mais il est introuvable.", client_id);
                    }
                }

                _ => warn!("Tag non reconnu : 0x{:02X}", tag),
            }
        }
        _ => {}
    }
}