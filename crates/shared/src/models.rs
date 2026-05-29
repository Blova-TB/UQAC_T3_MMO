use serde::{Deserialize, Serialize};
use bitcode::{Decode, Encode};
use mathtools::Vec2;
use crate::{define_packet, define_packet_router};
pub use crate::models_trait_and_macro::{BinaryField, ServerBinaryPacket};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "UPPERCASE")]
pub enum Status {
    Starting,
    Empty,
    Online,
    Full,
    Closed,
}

// List of packets -----------------------------------------------------------

// /!\ Lors de la définition d'un packet, il ne peut y avoir d'un seul chanmp non sizé (comme Vec<u8>)
// et il doit être le dernier champ de la structure.

define_packet_router! {
    pub enum SpatialServerPacket {
        Subscribe(Subscribe),
        Unsubscribe(Unsubscribe),
        Publish(Publish),
        Broadcast(Broadcast),
        ClientInput(ClientInput),
        Position(PositionUpdate),
        Subdivide(SubdivideUpdate),
        PlayerJoin(PlayerJoinUpdate),
        HandoffRequest(HandoffRequest),
        HandoffAccept(HandoffAccept),
        HandoffReject(HandoffReject),
        GhostUpdate(GhostUpdate),
        HandoffComplete(HandoffComplete),
    }
}

define_packet! {
    Subscribe(0x01) {
        client_id: u32,
        shard_id: u32,
    }
}

define_packet! {
    Unsubscribe(0x02) {
        client_id: u32,
        shard_id: u32,
    }
}

define_packet! {
    Publish(0x03) {
        shard_id: u32,
        payload: Vec<u8>,
    }
}

define_packet!{
    Broadcast(0x04) {
        payload: Vec<u8>,
    }
}

define_packet!{
    ClientInput(0x05) {
        client_id: u32,
        input_data: [u8; 16],
    }
}

define_packet! {
    PositionUpdate(0x10) {
        client_id: u32,
        pos: Vec2<f32>,
    }
}

define_packet!{
    SubdivideUpdate(0x11) {
        shard_id: u32,
    }
}

define_packet!{
    PlayerJoinUpdate(0x12) {
        client_id: u32,
        pos: Vec2<f32>,
    }
}

define_packet!{
    HandoffRequest(0x20) {
        entity_id: u32,
        pos: Vec2<f32>,
        vel: Vec2<f32>,
        state: [u8; 64],
    }
}

define_packet!{
    HandoffAccept(0x21) {
        entity_id: u32,
    }
}

define_packet!{
    HandoffReject(0x22) {
        entity_id: u32,
    }
}

define_packet!{
    GhostUpdate(0x23) {
        entity_id: u32,
        pos: Vec2<f32>,
        vel: Vec2<f32>,
    }
}

define_packet!{
    HandoffComplete(0x24) {
        entity_id: u32,
    }
}

define_packet!{
    SpawnServer(0x30) {
        shard_id: u32,
        pos_min: Vec2<f32>,
        pos_max: Vec2<f32>,
    }
}

define_packet!{
    ShutdownServer(0x31) {
        shard_id: u32,
    }
}

// ==========================================
//      PROTOCOLE RÉSEAU (CLIENT <-> SHARD)
// ==========================================

/// Paquets envoyés par le Client (ex: lors de l'initialisation ou via des inputs complexes)
#[derive(Debug, Clone, Encode, Decode)]
pub enum ClientPacket {
    Join { username: String },
    // Tu pourras ajouter ici:
    // MoveInput { x: f32, y: f32 },
    // UseSkill { skill_id: u8 },
}

/// Paquets événementiels envoyés par le Serveur vers un Client spécifique
#[derive(Debug, Clone, Encode, Decode)]
pub enum ServerPacket {
    Welcome { player_id: u32 }, // ⚠️ Passé en u32 pour matcher l'architecture du TP !
    RejectedFull,
    // Tu pourras ajouter ici:
    // ChatMessage { sender: String, msg: String },
}

// ==========================================
//      PROTOCOLE RÉSEAU (SYNCHRONISATION)
// ==========================================

/// Le payload interne encodé dans les Broadcasts (Tag 0x03 Publish / 0x04 Broadcast)
#[derive(Debug, Clone, Encode, Decode)]
pub struct ServerSyncMessage {
    pub players: Vec<PlayerPositionData>,
}

/// Représente l'état spatial d'une entité (sérialisé le plus petit possible)
#[derive(Debug, Clone, Encode, Decode)]
pub struct PlayerPositionData {
    pub entity_bits: u64,   // ID interne à l'ECS Bevy
    pub position: [f32; 2], // [x, y]
}