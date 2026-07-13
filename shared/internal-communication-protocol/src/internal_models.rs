pub use crate::internal_models_tools::{BinaryField, ServerBinaryPacket};
use custom_id::custom_id::CustomId;
use crate::{define_packet, define_packet_router};
use mathtools::Vec2;
use serde::{Deserialize, Serialize};



pub const STREAM_PHYSICS: u16 = 2;
pub const STREAM_HEARTBEAT: u16 = 3;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "UPPERCASE")]
pub enum Status {
    Starting,
    Empty,
    Online,
    Full,
    Closed,
    Waiting,
}

// List of packets -----------------------------------------------------------

define_packet_router! {
    pub enum CustomServerPacket {
        Subscribe(Subscribe),
        Unsubscribe(Unsubscribe),
        Publish(Publish),
        Broadcast(Broadcast),
        ClientInput(ClientInput),
        PositionUpdate(PositionUpdate),
        PlayerJoinUpdate(PlayerJoinUpdate),
        HandoffRequest(HandoffRequest),
        HandoffAccept(HandoffAccept),
        HandoffDrop(HandoffDrop),
        HandoffComplete(HandoffComplete),
        SpawnServer(SpawnServer),
        ServerSpawned(ServerSpawned),
        ServerHeartBeat(ServerHeartBeat),
        AssignShard(AssignShard),
        SpawnPlayerShard(SpawnPlayerShard),
        RefuseClient(RefuseClient),
        ClientLeft(ClientLeft),
        AoiPosUpdate(AoiPosUpdate),
        AoiModeChange(AoiModeChange),
    }
}

// ========================================== 0x00 : BROKER ==========================================

define_packet! {
    Subscribe(0x01) {
        custom_id: CustomId,
        topic_id: u32,
    }
}

define_packet! {
    Unsubscribe(0x02) {
        custom_id: CustomId,
        topic_id: u32,
    }
}

define_packet! {
    Publish(0x03) {
        topic_id: CustomId,
        payload: Vec<u8>,
    }
}

define_packet! {
    Broadcast(0x04) {
        payload: Vec<u8>,
    }
}

define_packet! {
    BroadcastClient(0x05) {
        client_id: CustomId,
        payload: Vec<u8>,
    }
}

// envoyé par le client au broker pour lui donner son jwt_token d'authentification qui a recu du GateKeeper
define_packet! {
    BrokerHandshakeClient(0x06) {
        jwt_token: Vec<u8>,
    }
}

define_packet! {
    BrokerHandshakeShard(0x07) {
        shard_id: CustomId,
    }
}

define_packet! {
    BrokerHandshakeSpatial(0x08) {
        magic: u16,
    }
}
define_packet!{
    BrokerHandshakeAoi(0x09) {
        magic: u16,
    }
}


// ========================================== 0x10 : SPATIAL ==========================================

// envoyé par le Broker au spatial apres voir recu un handshake du client
define_packet! {
    PlayerJoinUpdate(0x10) {
        client_id: CustomId,
        pos: Vec2<f32>,
    }
}

// envoyé par les shards au spatial pour update la position d'un client
define_packet! {
    PositionUpdate(0x11) {
        client_id: CustomId,
        pos: Vec2<f32>,
    }
}

// envoyé par le spatial au shard pour lui demander de spawn en ghost un client
define_packet! {
    HandoffRequest(0x13) {
        shard_id: CustomId,
        entity_id: CustomId,
    }
}

// envoyé par les shards au spatial pour dire qu'ils ont bien fait spawn le client concerné en ghost
define_packet! {
    HandoffAccept(0x12) {
        shard_id: CustomId,
        entity_id: CustomId,
    }
}

// envoyé par le spatial au shard pour lui dire de ne plus gerer le client (ghost) concerné
define_packet! {
    HandoffDrop(0x14) {
        shard_id: CustomId,
        entity_id: CustomId,
    }
}

// envoyé par le spatial au shard pour dire de prendre la main sur le client concerné (ghost -> player)
define_packet! {
    HandoffComplete(0x15) {
        new_shard_id: CustomId,
        old_shard_id: CustomId,
        entity_id: CustomId,
        pos: Vec2<f32>
    }
}

define_packet!{
    TakeAuthority(0x16) {
        entity_id: CustomId,
        pos: Vec2<f32>
    }
}

define_packet!{
    DropAuthority(0x17) {
        entity_id: CustomId,
    }
}

// ========================================== 0x20 : ORCHESTRATOR ==========================================

// appelé par le spatial pour demander de spawn un shard à une position donnée
define_packet! {
    SpawnServer(0x20) {
        shard_id: CustomId,
    }
}

// envoyé par le spatial pour dire que le shard est entrain de se vider et que le serveur va être shutdown une fois que tous les joueurs seront partis
define_packet! {
    ShutdownServerOnEmpty(0x21) {
        shard_id: CustomId,
    }
}

// envoyé au spatial pour confirmer que le shard est bien spawn
define_packet! {
    ServerSpawned(0x22) {
        shard_id: CustomId,
    }
}

// ========================================== 0x30 : SHARD (Game Server) ==========================================

// envoyé par le shard au spatial pour dire qu'il doit faire spawn ce client (probablement tout juste connecté)
define_packet! {
    SpawnPlayerShard(0x30) {
        shard_id: CustomId,
        client_id: CustomId,
        pos: Vec2<f32>,
    }
}

// envoyé par le shard au spatial pour dire qu'il doit faire despawn ce client
define_packet! {
    DespawnPlayerShard(0x31) {
        shard_id: CustomId,
        client_id: CustomId,
    }
}

// envoyé par le shard notamment au spatial pour la gestion du QuadTree (merge/split)
define_packet! {
    ServerHeartBeat(0x32) {
        shard_id: CustomId,
        occupancy: u8,
    }
}

// envoyé par l'orchestrateur pour dire à un shard de prendre en charge une zone du monde
define_packet! {
    AssignShard(0x33) {
        shard_id: CustomId,
    }
}

// envoyé par le Broker au shard pour lui dire qu'un client vient de se déconnecter
define_packet! {
    ClientLeft(0x34) {
        client_id: CustomId,
    }
}

// ========================================== 0x40 : CLIENT ==========================================

// envoyé par le client au shard pour lui envoyer des inputs (ex: déplacement, actions, etc.)
define_packet! {
    ClientInput(0x41) {
        client_id: CustomId,
        input_data: [u8; 16],
    }
}

define_packet!{
    RefuseClient(0x42) {
        client_id: CustomId,
    }
}


// ========================================== 0x50 : AOI SERVICE ==========================================

define_packet!{
    AoiPosUpdate(0x50) {
        client_id: CustomId,
        chunk_id: CustomId,
    }
}

define_packet!{
    AoiModeChange(0x51) {
        client_id: CustomId,
        chunk_id: CustomId,
        new_mode: u8,
    }
}