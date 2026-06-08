pub use crate::custom_id::CustomId;
pub use crate::models_trait_and_macro::{BinaryField, ServerBinaryPacket};
use crate::{define_packet, define_packet_router};
pub use mathtools::Vec2;
use serde::{Deserialize, Serialize};

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
        GhostUpdate(GhostUpdate),
        HandoffComplete(HandoffComplete),
        SpawnServer(SpawnServer),
        ServerSpawned(ServerSpawned),
        ShutdownServer(ShutdownServer),
        ServerHeartBeat(ServerHeartBeat),
        AssignShard(AssignShard),
        SpawnPlayerShard(SpawnPlayerShard),
        RefuseClient(RefuseClient),
        ClientLeft(ClientLeft),
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
        magic: u32,
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
        pos: Vec<f32>
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
        pos_min: Vec2<f32>,
        pos_max: Vec2<f32>,
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

// ========================================== 0x70 : Non Utilisés ? ==========================================

define_packet! {
    ShutdownServer(0x70) {
        shard_id: CustomId,
    }
}

define_packet! {
    GhostUpdate(0x71) {
        entity_id: CustomId,
        pos: Vec2<f32>,
        vel: Vec2<f32>,
    }
}

// // ==========================================
// //      PROTOCOLE RÉSEAU (CLIENT <-> SHARD)
// // ==========================================
//
// /// Paquets envoyés par le Client (ex: lors de l'initialisation ou via des inputs complexes)
// #[derive(Debug, Clone, Encode, Decode)]
// pub enum ClientPacket {
//     Join { username: String },
//     // Tu pourras ajouter ici:
//     // MoveInput { x: f32, y: f32 },
//     // UseSkill { skill_id: u8 },
// }
//
// /// Paquets événementiels envoyés par le Serveur vers un Client spécifique
// #[derive(Debug, Clone, Encode, Decode)]
// pub enum ServerPacket {
//     Welcome { player_id: u32 }, // ⚠️ Passé en u32 pour matcher l'architecture du TP !
//     RejectedFull,
//     // Tu pourras ajouter ici:
//     // ChatMessage { sender: String, msg: String },
// }
//
// // ==========================================
// //      PROTOCOLE RÉSEAU (SYNCHRONISATION)
// // ==========================================
//
// /// Le payload interne encodé dans les Broadcasts (Tag 0x03 Publish / 0x04 Broadcast)
// #[derive(Debug, Clone, Encode, Decode)]
// pub struct ServerSyncMessage {
//     pub players: Vec<PlayerPositionData>,
// }
//
// /// Représente l'état spatial d'une entité (sérialisé le plus petit possible)
// #[derive(Debug, Clone, Encode, Decode)]
// pub struct PlayerPositionData {
//     pub entity_bits: u64,   // ID interne à l'ECS Bevy
//     pub position: [f32; 2], // [x, y]
// }
