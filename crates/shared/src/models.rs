use serde::{Deserialize, Serialize};
use bitcode::{Decode, Encode};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "UPPERCASE")]
pub enum Status {
    Starting,
    Empty,
    Online,
    Full,
    Closed,
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