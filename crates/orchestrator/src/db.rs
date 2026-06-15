use redis::aio::MultiplexedConnection;
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use anyhow::{Result};

use internal_communication_protocol::internal_models::Status;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ServerInfo {
    pub server_id: String,
    pub shard_id: Option<u32>,
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

    pub async fn save_server(&self, server: &ServerInfo) -> Result<()> {
        let mut conn = self.conn.clone();
        let json = serde_json::to_string(server)?;

        let _: () = conn.hset("servers_hash", &server.server_id, json).await?;

        let shadow_key = format!("heartbeat:{}", server.server_id);
        let _: () = conn.set_ex(shadow_key, "1", 15).await?;

        Ok(())
    }

    pub async fn remove_server(&self, server_id: &str) -> Result<()> {
        let mut conn = self.conn.clone();
        let _: () = conn.hdel("servers_hash", server_id).await?;

        let shadow_key = format!("heartbeat:{}", server_id);
        let _: () = conn.del(shadow_key).await?;
        Ok(())
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