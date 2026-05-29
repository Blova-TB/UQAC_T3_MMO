use ahash::AHashMap;
use bytes::{Buf, BufMut, BytesMut};
use std::time::Duration;
use tracing::{error, info, warn};

// Le broker n'importe QUE la couche réseau (pas les modèles de jeu !)
use shared::network::protocols::QuicBackend;
use shared::network::{GameConnection, GameNetworkEvent, GamePeer};

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
    shard_conns: AHashMap<u32, GameConnection>,
    routing_table: OptimizedRoutingTable,

    // === 🛠️ DEBUG MOCK SPATIAL SERVER ===
    debug_default_topic: Option<u32>,
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
        GameNetworkEvent::Connected(conn) => {
            info!("Nouvelle connexion anonyme établie : {:?}", conn.connection_id);
        }
        GameNetworkEvent::Disconnected(conn) => {
            if let Some(client_id) = state.conn_to_client.remove(&conn) {
                // NOUVEAU : On prévient la Shard AVANT de supprimer l'abonnement
                if let Some(topic) = state.routing_table.get_topic_for_client(client_id) {
                    if let Some(shard_conn) = state.shard_conns.get(&topic) {
                        let mut notify_msg = BytesMut::with_capacity(5);
                        notify_msg.put_u8(0x07); // Tag 0x07: ClientLeft
                        notify_msg.put_u32_le(client_id);

                        // On utilise un stream Unreliable générique car on n'a plus le flux d'origine
                        let stream = shared::network::GameStream::new(0, shared::network::GameStreamReliability::Unreliable);
                        let _ = peer.send(shard_conn, &stream, notify_msg.freeze());
                    }
                }

                state.client_to_conn.remove(&client_id);
                state.routing_table.unsubscribe(client_id);
                info!("👤 Client {} déconnecté et désabonné.", client_id);
            }
            state.shard_conns.retain(|_, v| v != &conn);
        }
        GameNetworkEvent::Message { connection, stream, mut data } => {
            if data.is_empty() { return; }
            let tag = data.get_u8();

            match tag {
                // Handshake (Shard ou Client)
                0x00 => {
                    if data.remaining() < 5 { return; }
                    let is_shard = data.get_u8() == 1;
                    let id = data.get_u32_le(); // C'est notre CustomId !

                    if is_shard {
                        let shard_id = id; // L'ID *EST* le topic !
                        state.shard_conns.insert(shard_id, connection);
                        info!("🌍 Shard enregistrée pour le ShardId {:?}", shard_id);

                        // === 🛠️ DEBUG MOCK SPATIAL SERVER ===
                        if state.debug_default_topic.is_none() {
                            state.debug_default_topic = Some(shard_id);
                            warn!("🛠️ [DEBUG] La Shard {:?} est la Shard Globale par défaut !", shard_id);

                            let connected_clients: Vec<u32> = state.client_to_conn.keys().copied().collect();
                            for client_id in connected_clients {
                                state.routing_table.subscribe(client_id, shard_id);
                            }
                        }
                    } else {
                        state.conn_to_client.insert(connection, id);
                        state.client_to_conn.insert(id, connection);
                        info!("👤 Client {} authentifié.", id);

                        if let Some(default_topic) = state.debug_default_topic {
                            state.routing_table.subscribe(id, default_topic);

                            if let Some(shard_conn) = state.shard_conns.get(&default_topic) {
                                let mut notify_msg = BytesMut::with_capacity(5);
                                notify_msg.put_u8(0x06); // ClientJoined
                                notify_msg.put_u32_le(id);
                                let _ = peer.send(shard_conn, &stream, notify_msg.freeze());
                            }
                        }
                    }
                }

                // Subscribe
                0x01 => {
                    if data.remaining() < 8 { return; } // 4 (client_id) + 4 (topic)
                    let client_id = data.get_u32_le();
                    let topic = data.get_u32_le();
                    state.routing_table.subscribe(client_id, topic);
                }

                // Unsubscribe
                0x02 => {
                    if data.remaining() < 4 { return; }
                    let client_id = data.get_u32_le();
                    state.routing_table.unsubscribe(client_id);
                }

                // Publish/Broadcast (de Shard -> vers Clients)
                0x03 => {
                    if data.remaining() < 6 { return; } // 4 (topic) + 2 (len)
                    let topic = data.get_u32_le();
                    let payload_len = data.get_u16_le() as usize;

                    if data.remaining() < payload_len { return; }
                    let payload = data.copy_to_bytes(payload_len);

                    let mut broadcast_msg = BytesMut::with_capacity(3 + payload_len);
                    broadcast_msg.put_u8(0x04);
                    broadcast_msg.put_u16_le(payload_len as u16);
                    broadcast_msg.put(payload);
                    let final_msg = broadcast_msg.freeze();

                    if let Some(subscribers) = state.routing_table.get_subscribers(&topic) {
                        for client_id in subscribers {
                            if let Some(client_conn) = state.client_to_conn.get(client_id) {
                                let _ = peer.send(client_conn, &stream, final_msg.clone());
                            }
                        }
                    }
                }

                0x05 => {
                    if data.remaining() < 20 { return; }

                    let Some(client_id) = state.conn_to_client.get(&connection) else { return };
                    let Some(topic) = state.routing_table.get_topic_for_client(*client_id) else { return };
                    let Some(shard_conn) = state.shard_conns.get(&topic) else { return };

                    let mut forward_msg = BytesMut::with_capacity(21);
                    forward_msg.put_u8(0x05);
                    forward_msg.put_u32_le(*client_id);
                    forward_msg.put(data.copy_to_bytes(16));

                    let _ = peer.send(shard_conn, &stream, forward_msg.freeze());
                }

                _ => warn!("Tag non reconnu ignoré : 0x{:02X}", tag),
            }
        }
        _ => {}
    }
}