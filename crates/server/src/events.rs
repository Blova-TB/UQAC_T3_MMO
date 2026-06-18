use bevy::prelude::*;
use mathtools::Vec2 as MathVec2;

use custom_id::custom_id::CustomId;
use aoi_model::aoi_model::AoiMode;
use client_communication_protocol::client_models::{PlayerInputPayload,WorldSyncPayload};
use custom_id::chunk_id::ChunkId;

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
    SendWorldSync {
        chunk_id: ChunkId,
        world_sync_payload: WorldSyncPayload,
    },
    SendPositionUpdate {
        client_id: CustomId,
        pos: MathVec2<f32>,
    },
    SendHandoffAccept {
        shard_id: CustomId,
        entity_id: CustomId,
    },
    SendAoiModeChange {
        client_id: CustomId,
        pos: Vec2,
        mode: AoiMode,
    },
    AoiPosUpdate{
        client_id: CustomId,
        pos: Vec2,
    },
}

