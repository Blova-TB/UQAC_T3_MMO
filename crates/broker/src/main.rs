use ahash::AHashMap;
use bytes::{Buf, BufMut, BytesMut};
use std::time::Duration;
use tracing::{error, info, warn};
use mathtools::Vec2;

use shared::models::*;
use shared::network::protocols::QuicBackend;
use shared::network::{GameConnection, GameNetworkEvent, GamePeer};

use jsonwebtoken::{decode, DecodingKey, Validation};
use serde::{Deserialize, Serialize};

// 1. On reproduit la structure des Claims du Gatekeeper
#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,
    pub custom_id: u32,
    pub pos_x: f32,
    pub pos_y: f32,
    pub exp: usize,
}

// ==========================================
//      TABLE DE ROUTAGE OPTIMISÉE
// ==========================================

struct ClientRoutingMeta {
    topic: u32,
    index_in_vec: usize,
}

#[derive(Default)]
struct OptimizedRoutingTable {
    topic_subscribers: AHashMap<u32, Vec<u32>>,
    client_meta: AHashMap<u32, ClientRoutingMeta>,
}

impl OptimizedRoutingTable {
    pub fn subscribe(&mut self, client_id: u32, topic: u32) {
        self.unsubscribe(client_id);

        let subs = self.topic_subscribers.entry(topic).or_default();
        let index = subs.len();
        subs.push(client_id);

        self.client_meta.insert(client_id, ClientRoutingMeta { topic, index_in_vec: index });
    }

    pub fn unsubscribe(&mut self, client_id: u32) {
        if let Some(meta) = self.client_meta.remove(&client_id) {
            if let Some(subs) = self.topic_subscribers.get_mut(&meta.topic) {
                subs.swap_remove(meta.index_in_vec);
                if meta.index_in_vec < subs.len() {
                    let swapped_client_id = subs[meta.index_in_vec];
                    if let Some(swapped_meta) = self.client_meta.get_mut(&swapped_client_id) {
                        swapped_meta.index_in_vec = meta.index_in_vec;
                    }
                }
            }
        }
    }

    pub fn get_subscribers(&self, topic: &u32) -> Option<&[u32]> {
        self.topic_subscribers.get(topic).map(|v| v.as_slice())
    }

    pub fn get_topic_for_client(&self, client_id: u32) -> Option<u32> {
        self.client_meta.get(&client_id).map(|m| m.topic)
    }
}

// ==========================================
//      ÉTAT GLOBAL DU BROKER
// ==========================================

#[derive(Default)]
struct BrokerState {
    conn_to_client: AHashMap<GameConnection, u32>,
    client_to_conn: AHashMap<u32, GameConnection>,
    routing_table: OptimizedRoutingTable,

    spatial_server_conn: Option<GameConnection>,
    spatial_server_stream: Option<shared::network::GameStream>,

    shard_conns: AHashMap<u32, GameConnection>,
    shard_streams: AHashMap<u32, shared::network::GameStream>,
}

fn main() {
    tracing_subscriber::fmt::init();
    info!("🚀 Démarrage du Broker PubSub Optimisé (AVEC MOCK SPATIAL)...");

    let mut state = BrokerState::default();

    let mut peer = GamePeer::new(QuicBackend::new());
    if let Err(e) = peer.listen("0.0.0.0", 5000) {
        error!("❌ Impossible de bind le port 5000: {:?}", e);
        return;
    }

    info!("✅ Broker en écoute sur 0.0.0.0:5000 (UDP/QUIC)");

    loop {
        match peer.poll() {
            Ok(Some(event)) => handle_network_event(&mut state, event, &mut peer),
            Ok(None) => std::thread::sleep(Duration::from_millis(1)),
            Err(e) => error!("❌ Erreur réseau critique du peer: {:?}", e),
        }
    }
}

// ==========================================
//      PARSER BINAIR (LITTLE-ENDIAN)
// ==========================================

fn handle_network_event(state: &mut BrokerState, event: GameNetworkEvent, peer: &mut GamePeer) {
    match event {
        GameNetworkEvent::Disconnected(conn) => {
            if let Some(client_id) = state.conn_to_client.remove(&conn) {
                // 🚀 On utilise le vrai paquet ClientLeft !
                if let Some(topic) = state.routing_table.get_topic_for_client(client_id) {
                    if let Some(shard_conn) = state.shard_conns.get(&topic) {
                        let left_pkt = ClientLeft { client_id: CustomId::from(client_id) };
                        let stream = shared::network::GameStream::new(0, shared::network::GameStreamReliability::Reliable);
                        let _ = peer.send(shard_conn, &stream, left_pkt.to_bytes());
                    }
                }
                state.client_to_conn.remove(&client_id);
                state.routing_table.unsubscribe(client_id);
                info!("👤 Client {} déconnecté et désabonné.", client_id);
            }
            state.shard_conns.retain(|_, v| v != &conn);

            if Some(conn) == state.spatial_server_conn {
                warn!("🚨 ALERTE : SPATIAL SERVER DÉCONNECTÉ ! 🚨");
                state.spatial_server_conn = None;
            }
        }

        GameNetworkEvent::Message { connection, stream, data } => {
            if data.is_empty() { return; }
            let tag = data[0]; // On lit juste le Tag sans consommer le buffer

            match tag {
                // === HANDSHAKES ===
                BrokerHandshakeClient::TAG => {
                    let Some(pkt) = BrokerHandshakeClient::try_from_bytes(data) else { return; };
                    let token_str = String::from_utf8_lossy(&pkt.jwt_token);
                    let secret = std::env::var("JWT_SECRET").unwrap_or_else(|_| "secret_de_dev".to_string());

                    match decode::<Claims>(
                        token_str.as_ref(),
                        &DecodingKey::from_secret(secret.as_bytes()),
                        &Validation::default(),
                    ) {
                        Ok(token_data) => {
                            let claims = token_data.claims;
                            let client_id = claims.custom_id;

                            state.conn_to_client.insert(connection.clone(), client_id);
                            state.client_to_conn.insert(client_id, connection);
                            info!("👤 Client {} authentifié (Pos: {}, {}).", client_id, claims.pos_x, claims.pos_y);

                            if let Some(spatial_conn) = &state.spatial_server_conn {
                                if let Some(spatial_stream) = &state.spatial_server_stream {
                                    let join_pkt = PlayerJoinUpdate {
                                        client_id: CustomId::from(client_id),
                                        pos: mathtools::Vec2::new(claims.pos_x, claims.pos_y),
                                    };
                                    let _ = peer.send(spatial_conn, spatial_stream, join_pkt.to_bytes());
                                }
                            } else {
                                warn!("⚠️ Spatial Server hors-ligne ! Joueur {} en attente.", client_id);
                            }
                        }
                        Err(e) => warn!("⚠️ JWT invalide : {:?}", e),
                    }
                }

                BrokerHandshakeShard::TAG => {
                    let Some(pkt) = BrokerHandshakeShard::try_from_bytes(data) else { return; };
                    let shard_id_u32: u32 = pkt.shard_id.into();

                    state.shard_conns.insert(shard_id_u32, connection);
                    state.shard_streams.insert(shard_id_u32, stream);

                    info!("🌍 Shard {} authentifiée.", shard_id_u32);
                }

                BrokerHandshakeSpatial::TAG => {
                    state.spatial_server_conn = Some(connection);
                    state.spatial_server_stream = Some(stream);

                    info!("🧠 Spatial Server authentifié !");
                }

                // === ROUTAGE STANDARD ===
                Subscribe::TAG => {
                    let Some(pkt) = Subscribe::try_from_bytes(data) else { return; };
                    state.routing_table.subscribe(pkt.client_id.into(), pkt.topic_id);
                }

                Unsubscribe::TAG => {
                    let Some(pkt) = Unsubscribe::try_from_bytes(data) else { return; };
                    state.routing_table.unsubscribe(pkt.client_id.into());
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
                    let client_id: u32 = pkt.client_id.into();
                    if let Some(topic) = state.routing_table.get_topic_for_client(client_id) {
                        if let Some(shard_conn) = state.shard_conns.get(&topic) {
                            // On fait suivre la trame brute directement (Zero-copy)
                            let _ = peer.send(shard_conn, &stream, data);
                        }
                    }
                }

                // 🚀 L'ORDRE D'APPARITION DU SPATIAL SERVEUR VERS LA SHARD
                MessageQueLeSpatialEnvoieAUnGameServerPourFairSpawnerUnJoueur_IlPrendDoncDirectementLAutoriteEtSubscribeAuInputsDuPlayer::TAG => {
                    let Some(pkt) = MessageQueLeSpatialEnvoieAUnGameServerPourFairSpawnerUnJoueur_IlPrendDoncDirectementLAutoriteEtSubscribeAuInputsDuPlayer::try_from_bytes(data.clone()) else { return; };
                    let shard_id: u32 = pkt.shard_id.into();

                    if let Some(shard_conn) = state.shard_conns.get(&shard_id) {
                        if let Some(shard_stream) = &state.shard_streams.get(&shard_id) {
                            let _ = peer.send(shard_conn, &shard_stream, data);
                        }
                    }
                }

                _ => warn!("Tag non reconnu : 0x{:02X}", tag),
            }
        }
        _ => {}
    }
}