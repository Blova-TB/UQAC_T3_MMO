use redis::aio::MultiplexedConnection;
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ServerInfo {
    pub container_id: String,
    pub address: String,
    pub players_online: u32,
    pub max_players: u32,
}

// 2. Le gestionnaire de DB. Clone est très peu coûteux ici car
// MultiplexedConnection est conçu pour être partagé entre les threads.
#[derive(Clone)]
pub struct Database {
    conn: MultiplexedConnection,
}

impl Database {
    pub async fn new(redis_url: &str) -> Result<Self, redis::RedisError> {
        // 1. Création du client (parsing de l'URL)
        let client = redis::Client::open(redis_url)?;

        // 2. Établissement de la connexion asynchrone unifiée
        let conn = client.get_multiplexed_async_connection().await?;

        Ok(Self { conn })
    }

    pub async fn save_server(&self, server: &ServerInfo) -> Result<(), redis::RedisError> {
        let mut conn = self.conn.clone(); // Requis par l'API de la crate redis
        let json = serde_json::to_string(server).unwrap(); // Serialisation ultra-rapide

        // On utilise l'ID du conteneur comme clé unique
        let key = format!("server:{}", server.container_id);
        conn.set(key, json).await
    }

    pub async fn get_server(&self, container_id: &str) -> Result<Option<ServerInfo>, redis::RedisError> {
        let mut conn = self.conn.clone();
        let key = format!("server:{}", container_id);

        let result: Option<String> = conn.get(key).await?;

        // Map la string JSON de retour vers notre Struct Rust
        Ok(result.map(|json| serde_json::from_str(&json).unwrap()))
    }
}