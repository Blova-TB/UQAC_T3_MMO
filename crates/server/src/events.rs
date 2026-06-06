use bevy::prelude::*;
use shared::custom_id::CustomId;
use shared::game_protocol::PlayerInputPayload;

#[derive(Message)]
pub enum BrokerEvent {
    SpawnPlayer {
        client_id: CustomId,
        pos: Vec2,
    },
    PlayerLeft {
        client_id: CustomId,
    },
    PlayerInput {
        client_id: CustomId,
        payload: PlayerInputPayload,
    },
}