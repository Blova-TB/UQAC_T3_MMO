use shared::network::{GameConnection, GameNetworkEvent, GamePeer, GameStream, GameStreamReliability};
use shared::network::protocols::QuicBackend;
use bytes::Bytes;
use shared::models::*;

pub struct InfrastructureNetwork {
    orchestrator_peer: GamePeer,
    orchestrator_conn: Option<GameConnection>,
    orchestrator_reliable_stream: Option<GameStream>,

    broker_peer: GamePeer,
    broker_conn: Option<GameConnection>,
    broker_reliable_stream: Option<GameStream>,
}

impl InfrastructureNetwork {
    pub fn new(orchestrator_addr: &str, orchestrator_port: u16, broker_addr: &str, broker_port: u16) -> Self {
        let orchestrator_peer = GamePeer::new(QuicBackend::new());
        let broker_peer = GamePeer::new(QuicBackend::new());

        // Initialisation stricte : le service doit paniquer s'il ne peut pas lier ses sockets locaux
        orchestrator_peer.connect(orchestrator_addr, orchestrator_port)
            .expect("Échec du bind socket vers l'Orchestrateur");

        broker_peer.connect(broker_addr, broker_port)
            .expect("Échec du bind socket vers le Broker");

        Self {
            orchestrator_peer,
            orchestrator_conn: None,
            orchestrator_reliable_stream: None,

            broker_peer,
            broker_conn: None,
            broker_reliable_stream: None,
        }
    }

    /// Envoi garanti vers l'Orchestrateur
    pub fn send_to_orchestrator(&self, data: Bytes) -> Result<(), String> {
        let conn = self.orchestrator_conn.as_ref().ok_or("Non connecté à l'Orchestrateur")?;
        let stream = self.orchestrator_reliable_stream.as_ref().ok_or("Flux Orchestrateur non établi")?;

        self.orchestrator_peer.send(conn, stream, data)
            .map_err(|e| format!("Erreur d'envoi Orchestrateur: {:?}", e))
    }

    /// Envoi garanti vers le Broker
    pub fn send_to_broker(&self, data: Bytes) -> Result<(), String> {
        let conn = self.broker_conn.as_ref().ok_or("Non connecté au Broker")?;
        let stream = self.broker_reliable_stream.as_ref().ok_or("Flux Broker non établi")?;

        self.broker_peer.send(conn, stream, data)
            .map_err(|e| format!("Erreur d'envoi Broker: {:?}", e))
    }

    /// Dépile les événements réseau. À appeler à chaque tick de la boucle principale.
    pub fn poll_events(&mut self) -> Vec<InfrastructureEvent> {
        let mut events = Vec::new();

        // 1. Polling Orchestrateur
        while let Ok(Some(event)) = self.orchestrator_peer.poll() {
            self.handle_peer_event(event, PeerType::Orchestrator, &mut events);
        }

        // 2. Polling Broker
        while let Ok(Some(event)) = self.broker_peer.poll() {
            self.handle_peer_event(event, PeerType::Broker, &mut events);
        }

        events
    }

    fn handle_peer_event(&mut self, event: GameNetworkEvent, peer_type: PeerType, output: &mut Vec<InfrastructureEvent>) {
        match event {
            GameNetworkEvent::Connected(conn) => {
                match peer_type {
                    PeerType::Orchestrator => {
                        self.orchestrator_conn = Some(conn.clone());
                        // Demande d'ouverture d'un flux bidirectionnel fiable
                        let _ = self.orchestrator_peer.create_stream(conn, GameStreamReliability::Reliable);
                    }
                    PeerType::Broker => {
                        self.broker_conn = Some(conn.clone());
                        let _ = self.broker_peer.create_stream(conn, GameStreamReliability::Reliable);
                    }
                }
            }
            GameNetworkEvent::StreamCreated(conn, stream) => {
                if stream.is_reliable() {
                    match peer_type {
                        PeerType::Orchestrator => self.orchestrator_reliable_stream = Some(stream),
                        PeerType::Broker => {
                            self.broker_reliable_stream = Some(stream.clone());
                            let handshake = BrokerHandshakeSpatial { magic: 42 };
                            let _ = self.broker_peer.send(&conn, &stream, handshake.to_bytes());
                            println!("🧠 [Spatial] Handshake typé envoyé au Broker !");
                        }
                    }
                }
            }
            GameNetworkEvent::Message { data, .. } => {
                output.push(InfrastructureEvent::MessageReceived { source: peer_type, data });
            }
            GameNetworkEvent::Disconnected(_) | GameNetworkEvent::Error { .. } => {
                match peer_type {
                    PeerType::Orchestrator => {
                        self.orchestrator_conn = None;
                        self.orchestrator_reliable_stream = None;
                    }
                    PeerType::Broker => {
                        self.broker_conn = None;
                        self.broker_reliable_stream = None;
                    }
                }
                output.push(InfrastructureEvent::Disconnected { source: peer_type });
            }
            _ => {}
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum PeerType {
    Orchestrator,
    Broker,
}

pub enum InfrastructureEvent {
    MessageReceived { source: PeerType, data: Bytes },
    Disconnected { source: PeerType },
}