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

define_packet_router! {
    pub enum SpatialServerPacket {
        Subscribe(Subscribe),
        Unsubscribe(Unsubscribe),
        Position(PositionUpdate),
        Subdivide(SubdivideUpdate),
        PlayerJoin(PlayerJoinUpdate),
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