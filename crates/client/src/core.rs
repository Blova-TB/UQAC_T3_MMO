use bevy::prelude::*;
use tokio::runtime::Runtime;

use client_communication_protocol::client_models::*;
use network_protocol::network::{GameConnection, GamePeer};

pub struct CorePlugin;

impl Plugin for CorePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TokioRuntime>()
            .add_message::<NetworkSnapshotEvent>()
            .add_systems(Startup, setup_core);
    }
}

#[derive(Resource)]
pub struct TokioRuntime(pub Runtime);

impl Default for TokioRuntime {
    fn default() -> Self {
        Self(
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("Échec de l'initialisation du Runtime Tokio"),
        )
    }
}

#[derive(Resource)]
pub struct SessionData {
    pub gatekeeper_token: String,
    pub session_token: String,
    pub custom_id: u32,
}

#[derive(Resource)]
pub struct TargetServer {
    pub ip: String,
    pub port: u16,
}

#[derive(Resource)]
pub struct ClientState {
    pub peer: GamePeer,
    pub connection: Option<GameConnection>,
}

#[derive(Resource)]
pub struct GameAssets {
    pub player_sprite: Handle<Image>,
}

#[derive(Message)]
pub struct NetworkSnapshotEvent {
    pub players: Vec<PlayerData>,
}

fn setup_core(mut commands: Commands, asset_server: Res<AssetServer>) {
    commands.insert_resource(GameAssets {
        player_sprite: asset_server.load("circle.png"),
    });
}