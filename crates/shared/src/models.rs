use serde::{Deserialize, Serialize};
use mathtools::Vec2;
use crate::{define_packet, define_packet_router};
use crate::models_trait_and_macro::{BinaryField, SpatialServerBinaryPacket};

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