use bevy::prelude::*;
use shared::custom_id::CustomId;
use shared::game_protocol::PlayerInputPayload;
use shared::game_protocol::WorldSyncPayload;
use shared::models::Vec2 as MathVec2;

#[derive(Message)]
pub enum BrokerEvent {
    SpawnPlayer {
        client_id: CustomId,
        pos: Vec2,
    },
    SpawnGhostPlayer {
        shard_id: CustomId,
        client_id: CustomId,
    },
    PlayerLeft {
        client_id: CustomId,
    },
    PlayerInput {
        client_id: CustomId,
        payload: PlayerInputPayload,
    },
    TakeAuthority {
        client_id: CustomId,
    },
    DropAuthority {
        client_id: CustomId,
    },
}

#[derive(Message)]
pub enum BrokerCommand {
    SendWorldSync(WorldSyncPayload),
    SendPositionUpdate {
        client_id: CustomId,
        pos: MathVec2<f32>,
    },
    SendHandoffAccept {
        shard_id: CustomId,
        entity_id: CustomId,
    },
}

