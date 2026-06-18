use ahash::AHashMap;
use tracing::{error, info, warn};
use jsonwebtoken::{decode, DecodingKey, Validation};
use serde::{Deserialize, Serialize};
use mathtools::Vec2;

use network_protocol::network::protocols::QuicBackend;
use network_protocol::network::{GameConnection, GameNetworkEvent, GamePeer, GameStream, GameStreamReliability};
use internal_communication_protocol::internal_models::*;
use custom_id::custom_id::CustomId;

mod routing;
use routing::OptimizedRoutingTable;


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

    aoi_server_conn: Option<GameConnection>,
    aoi_server_stream: Option<GameStream>,

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
            aoi_server_conn: None,
            aoi_server_stream: None,
            shard_conns: AHashMap::new(),
            shard_streams: AHashMap::new(),
        }
    }
}

impl BrokerState {
    pub fn handle_disconnect(&mut self, peer: &mut GamePeer, conn: GameConnection) {
        if let Some(client_id) = self.conn_to_client.remove(&conn) {

            let packet = ClientLeft {
                client_id: CustomId::from(client_id),
            }.to_bytes();

            if let Some(spatial_conn) = &self.spatial_server_conn {
                if let Some(spatial_stream) = &self.spatial_server_stream {
                    let _ = peer.send(spatial_conn, spatial_stream, packet);
                }
            }

            let _ = self.routing_table.unsubscribe_all(client_id);
            self.routing_table.remove_topic(client_id);

            self.client_to_conn.remove(&client_id);
            self.client_streams.remove(&client_id);

            info!("👤 Client {} déconnecté. Spatial Server notifié.", client_id);
            return;
        }

        if self.spatial_server_conn.as_ref() == Some(&conn) {
            self.spatial_server_conn = None;
            self.spatial_server_stream = None;
            warn!("🗺️ Spatial Server déconnecté !");
            return;
        }

        if self.aoi_server_conn.as_ref() == Some(&conn) {
            self.aoi_server_conn = None;
            self.aoi_server_stream = None;
            warn!("👁️ AOI Server déconnecté !");
            return;
        }

        let disconnected_shard_id = self.shard_conns.iter()
            .find_map(|(&id, c)| (c == &conn).then_some(id));

        if let Some(shard_id) = disconnected_shard_id {
            if let Some(orphaned_clients) = self.routing_table.remove_topic(shard_id) {
                if !orphaned_clients.is_empty() {
                    warn!(
                        "⚠️ Shard {} déconnectée de force ! {} joueurs orphelins : {:?}",
                        shard_id, orphaned_clients.len(), orphaned_clients
                    );
                }
            }

            self.routing_table.unsubscribe_all(shard_id);

            self.shard_conns.remove(&shard_id);
            self.shard_streams.remove(&shard_id);

            info!("🧩 Shard {} déconnectée et nettoyée.", shard_id);
            return;
        }

        warn!("❓ Déconnexion d'une entité non répertoriée dans le BrokerState.");
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
            state.handle_disconnect(peer, conn);
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
                    println!("Handshake Spatial reçu, connexion établie avec le Spatial Server !");
                    state.spatial_server_conn = Some(connection);
                    state.spatial_server_stream = Some(stream);
                }

                BrokerHandshakeAoi::TAG => {
                    println!("Handshake AOI reçu, connexion établie avec l'AOI Server !");
                    state.aoi_server_conn = Some(connection);
                    state.aoi_server_stream = Some(stream);
                }

                Subscribe::TAG => {
                    // println!("Subscribe received : ");
                    let Some(pkt) = Subscribe::try_from_bytes(data) else { return; };
                    // println!("custom_id = {}, topic_id = {}", pkt.custom_id.0, pkt.topic_id);
                    state.routing_table.subscribe(pkt.custom_id.into(), pkt.topic_id);
                }

                Unsubscribe::TAG => {
                    // println!("Unsubscribe received : ");
                    let Some(pkt) = Unsubscribe::try_from_bytes(data) else { return; };
                    // println!("custom_id = {}, topic_id = {}", pkt.custom_id.0, pkt.topic_id);
                    state.routing_table.unsubscribe(pkt.custom_id.into(), pkt.topic_id);
                }

                Publish::TAG => {
                    // println!("Publish received : ");
                    let Some(pkt) = Publish::try_from_bytes(data) else { return; };
                    // println!("topic_id = {}, payload size = {}", pkt.topic_id.0, pkt.payload.len());
                    let topic_u32: u32 = pkt.topic_id.into();
                    let final_msg = if let Some(&sender_client_id) = state.conn_to_client.get(&connection) {
                        BroadcastClient {
                            client_id: CustomId::from(sender_client_id),
                            payload: pkt.payload,
                        }.to_bytes()
                    } else {
                        Broadcast {
                            payload: pkt.payload,
                        }.to_bytes()
                    };
                    if let Some(subscribers) = state.routing_table.get_subscribers(&topic_u32) {
                        for &sub_id in subscribers {
                            if let Some(client_conn) = state.client_to_conn.get(&sub_id) {
                                let _ = peer.send(client_conn, &stream, final_msg.clone());
                            }
                            else if let Some(shard_conn) = state.shard_conns.get(&sub_id) {
                                let _ = peer.send(shard_conn, &stream, final_msg.clone());
                            }
                        }
                    }
                }

                SpawnPlayerShard::TAG => {
                    let Some(pkt) = SpawnPlayerShard::try_from_bytes(data.clone()) else { return; };
                    let shard_id: u32 = pkt.shard_id.into();
                    let client_id: u32 = pkt.client_id.into();

                    // state.routing_table.subscribe(client_id, shard_id);
                    state.routing_table.subscribe(shard_id, client_id);

                    if let Some(conn) = state.shard_conns.get(&shard_id) {
                        let _ = peer.send(conn, state.shard_streams.get(&shard_id).unwrap(), data);
                    }
                }

                DespawnPlayerShard::TAG => {
                    let Some(pkt) = DespawnPlayerShard::try_from_bytes(data.clone()) else { return; };
                    let shard_id: u32 = pkt.shard_id.into();
                    let client_id: u32 = pkt.client_id.into();

                    // state.routing_table.unsubscribe(client_id, shard_id);
                    state.routing_table.unsubscribe(shard_id, client_id);

                    if let Some(conn) = state.shard_conns.get(&shard_id) {
                        let _ = peer.send(conn, state.shard_streams.get(&shard_id).unwrap(), ClientLeft {
                            client_id: CustomId::from(client_id)
                        }.to_bytes());
                    }
                }

                ServerHeartBeat::TAG => {
                    if let Some(s_conn) = &state.spatial_server_conn {
                        let unreliable_stream = GameStream::new(
                            STREAM_PHYSICS,
                            GameStreamReliability::Unreliable
                        );
                        let _ = peer.send(s_conn, &unreliable_stream, data);
                    } else {
                        warn!("⚠️ ServerHeartBeat ignoré : Spatial Server non connecté.");
                    }
                }

                PositionUpdate::TAG => {
                    println!("PositionUpdate received");
                    if let Some(s_conn) = &state.spatial_server_conn {
                        let unreliable_stream = GameStream::new(
                            STREAM_PHYSICS,
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

                HandoffRequest::TAG => {
                    println!("FastHandOff received");
                    let Some(pkt) = HandoffRequest::try_from_bytes(data.clone()) else { return; };

                    let shard_id: u32 = pkt.shard_id.into();
                    let entity_id: u32 = pkt.entity_id.into();
                    println!("shard_id : {:?}, entity_id : {:?}", shard_id, entity_id);

                    // on abonne le game server au input et a la position du client
                    state.routing_table.subscribe(shard_id, entity_id);

                    // on forward la requete de handoff au shard concerné
                    if let Some(conn) = state.shard_conns.get(&shard_id) {
                        let _ = peer.send(conn, state.shard_streams.get(&shard_id).unwrap(), data);
                    }
                }

                HandoffAccept::TAG  => {
                    if let (Some(s_conn),Some(s_stream)) = (&state.spatial_server_conn, &state.spatial_server_stream) {
                        let _ = peer.send(s_conn, s_stream, data);
                    } else {
                        warn!("⚠️ PositionUpdate ignoré : Spatial Server non connecté.");
                    }
                }

                HandoffDrop::TAG => {
                    let Some(pkt) = HandoffDrop::try_from_bytes(data.clone()) else { return; };

                    let shard_id: u32 = pkt.shard_id.into();
                    let entity_id: u32 = pkt.entity_id.into();

                    // on desabonne le game server du client (position et input)
                    state.routing_table.unsubscribe(shard_id, entity_id);

                    // on forward la requete de drop au shard concerné
                    if let Some(conn) = state.shard_conns.get(&shard_id) {
                        let _ = peer.send(conn, state.shard_streams.get(&shard_id).unwrap(), data);
                    }
                }

                HandoffComplete::TAG => {
                    let Some(pkt) = HandoffComplete::try_from_bytes(data.clone()) else { return; };

                    let new_shard_id: u32 = pkt.new_shard_id.into();
                    let old_shard_id: u32 = pkt.old_shard_id.into();
                    let new_pos: Vec2<f32> = pkt.pos.into();

                    if let Some(conn) = state.shard_conns.get(&new_shard_id) {
                        let take_pkt = TakeAuthority {
                            entity_id: pkt.entity_id,
                            pos: new_pos,
                        }.to_bytes();
                        let _ = peer.send(conn, state.shard_streams.get(&new_shard_id).unwrap(), take_pkt);
                    }

                    //state.routing_table.subscribe(pkt.entity_id.into(), new_shard_id);

                    if let Some(conn) = state.shard_conns.get(&old_shard_id) {
                        let drop_pkt = DropAuthority {
                            entity_id: pkt.entity_id,
                        }.to_bytes();
                        let _ = peer.send(conn, state.shard_streams.get(&old_shard_id).unwrap(), drop_pkt);
                    }

                    //state.routing_table.unsubscribe(pkt.entity_id.into(), old_shard_id);
                }

                ShutdownServerOnEmpty::TAG => {
                    let Some(pkt) = ShutdownServerOnEmpty::try_from_bytes(data.clone()) else { return; };
                    let shard_id: u32 = pkt.shard_id.into();
                    if let (Some(s_conn), Some(s_stream)) = (state.shard_conns.get(&shard_id), state.shard_streams.get(&shard_id)) {
                        let _ = peer.send(s_conn, s_stream, data);
                    }
                    println!("Shutdown server on empty")
                }

                AoiPosUpdate::TAG => {
                    if let Some(aoi_conn) = &state.aoi_server_conn {
                        let unreliable_stream = GameStream::new(
                            STREAM_PHYSICS,
                            GameStreamReliability::Unreliable
                        );
                        let _ = peer.send(aoi_conn, &unreliable_stream, data);
                    } else {
                        warn!("⚠️ PositionUpdate ignoré : Aoi Server non connecté.");
                    }
                }

                AoiModeChange::TAG => {
                    if let (Some(aoi_conn), Some(aoi_stream)) = (&state.aoi_server_conn, &state.aoi_server_stream) {
                        let _ = peer.send(aoi_conn, aoi_stream, data);
                    }
                }

                _ => warn!("Tag non reconnu : 0x{:02X}", tag),
            }
        }
        _ => {}
    }
}