use redis::aio::MultiplexedConnection;
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use shared::models::Status;
use anyhow::{Result};
use rocket::serde::json::serde_json;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ServerInfo {
    pub container_id: String,
    pub address: String,
    pub players_online: usize,
    pub max_players: usize,
    pub status: Status,
}

#[derive(Clone)]
pub struct Database {
    pub client: redis::Client,
    pub conn: MultiplexedConnection,
}

impl Database {
    pub async fn new(redis_url: &str) -> Result<Self> {
        let client = redis::Client::open(redis_url)?;
        let conn = client.get_multiplexed_async_connection().await?;
        Ok(Self { client, conn })
    }

    pub async fn get_all_servers(&self) -> Result<Vec<ServerInfo>> {
        let mut conn = self.conn.clone();
        let servers_json: Vec<String> = conn.hvals("servers_hash").await?;

        let servers = servers_json
            .into_iter()
            .filter_map(|json| serde_json::from_str::<ServerInfo>(&json).ok())
            .collect();

        Ok(servers)
    }
}