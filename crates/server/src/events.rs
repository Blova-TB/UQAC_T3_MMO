use bevy::prelude::*;

use custom_id::custom_id::CustomId;
use client_communication_protocol::client_models::{PlayerInputPayload,WorldSyncPayload};
use internal_communication_protocol::internal_models::Vec2 as MathVec2;

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
        new_pos: Vec2,
    },
    DropAuthority {
        client_id: CustomId,
    },
    HandoffDrop {
        client_id: CustomId,
    },
    ShutdownRequested,
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

