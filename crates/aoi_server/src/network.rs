use bytes::Bytes;

use network_protocol::network::{GameConnection, GameNetworkEvent, GamePeer, GameStream, GameStreamReliability};
use network_protocol::network::protocols::QuicBackend;
use internal_communication_protocol::internal_models::*;

pub struct InfrastructureNetwork {
    broker_peer: GamePeer,
    broker_conn: Option<GameConnection>,
    broker_reliable_stream: Option<GameStream>,
}

impl InfrastructureNetwork {
    pub fn new(broker_addr: &str, broker_port: u16) -> Self {
        let broker_peer = GamePeer::new(QuicBackend::new());

        broker_peer.connect(broker_addr, broker_port)
            .expect("Échec du bind socket vers le Broker");

        Self {
            broker_peer,
            broker_conn: None,
            broker_reliable_stream: None,
        }
    }

    pub fn send_to_broker(&self, data: Bytes) -> Result<(), String> {

        let Some(conn) = self.broker_conn.as_ref() else {
            println!("❌ Tentative d'envoi au Broker sans connexion établie !");
            return Err("Non connecté au Broker".to_string());
        };

        let Some(stream) = self.broker_reliable_stream.as_ref() else {
            println!("❌ Tentative d'envoi au Broker sans stream fiable établi !");
            return Err("Flux fiable du Broker non établi".to_string());
        };

        self.broker_peer.send(conn, stream, data)
            .map_err(|e| format!("Erreur d'envoi Broker: {:?}", e))
    }

    pub fn poll_events(&mut self) -> Vec<InfrastructureEvent> {
        let mut events = Vec::new();

        while let Ok(Some(event)) = self.broker_peer.poll() {
            self.handle_peer_event(event, &mut events);
        }

        events
    }

    fn handle_peer_event(&mut self, event: GameNetworkEvent, output: &mut Vec<InfrastructureEvent>) {
        match event {
            GameNetworkEvent::Connected(conn) => {
                self.broker_conn = Some(conn.clone());
                let _ = self.broker_peer.create_stream(conn, GameStreamReliability::Reliable);
            }
            GameNetworkEvent::StreamCreated(conn, stream) => {
                if stream.is_reliable() {
                    self.broker_reliable_stream = Some(stream.clone());
                    let handshake = BrokerHandshakeAoi { magic: 42 };
                    let _ = self.broker_peer.send(&conn, &stream, handshake.to_bytes());
                    println!("🧠 [Aoi] Handshake typé envoyé au Broker !");
                }
            }
            GameNetworkEvent::Message { data, .. } => {
                output.push(InfrastructureEvent::MessageReceived { data });
            }
            GameNetworkEvent::Disconnected(_) | GameNetworkEvent::Error { .. } => {

                self.broker_conn = None;
                self.broker_reliable_stream = None;

                output.push(InfrastructureEvent::Disconnected {});
            }
            _ => {}
        }
    }
}

pub enum InfrastructureEvent {
    MessageReceived { data: Bytes },
    Disconnected {},
}