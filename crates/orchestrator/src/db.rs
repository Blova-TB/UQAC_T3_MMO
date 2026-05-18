use redis::aio::MultiplexedConnection;
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use shared::models::Status;
use anyhow::{Context, Result};

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
    conn: MultiplexedConnection,
}

impl Database {
    pub async fn new(redis_url: &str) -> Result<Self> {
        let client = redis::Client::open(redis_url)
            .context("L'URL Redis est mal formatée")?;
        let conn = client.get_multiplexed_async_connection().await
            .context("Impossible d'établir la connexion TCP avec Redis")?;
        Ok(Self { conn })
    }

    pub async fn save_server(&self, server: &ServerInfo) -> Result<()> {
        let mut conn = self.conn.clone();
        let json = serde_json::to_string(server)?;
        let _: () = conn.hset("servers_hash", &server.container_id, json).await?;
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

    pub async fn remove_server(&self, container_id: &str) -> anyhow::Result<()> {
        let mut conn = self.conn.clone();
        let _: () = conn.hdel("servers_hash", container_id).await?;

        Ok(())
    }
}