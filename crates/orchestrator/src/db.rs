use redis::aio::MultiplexedConnection;
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum Status {
    Starting,
    Connected,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ServerInfo {
    pub container_id: String,
    pub address: String,
    pub players_online: u32,
    pub max_players: u32,
    pub status: Status,
}

#[derive(Clone)]
pub struct Database {
    conn: MultiplexedConnection,
}

impl Database {
    pub async fn new(redis_url: &str) -> Result<Self, redis::RedisError> {
        let client = redis::Client::open(redis_url)?;
        let conn = client.get_multiplexed_async_connection().await?;
        Ok(Self { conn })
    }

    /// Utilise HSET pour ranger le serveur dans un "dossier" global
    pub async fn save_server(
        &self,
        server: &ServerInfo,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut conn = self.conn.clone();

        // Sécurisation de la sérialisation
        let json = serde_json::to_string(server)?;

        // 🌟 CORRECTION ICI : On assigne le résultat à `_` en forçant le type `()`.
        // Cela dit au compilateur : "Exécute la requête, propage l'erreur si besoin avec `?`,
        // mais je me fiche de la valeur de retour (true/false), traite-la comme vide".
        let _: () = conn.hset("servers_hash", &server.container_id, json).await?;

        Ok(())
    }

    /// L'algorithme de Matchmaking (Recherche du meilleur serveur)
    pub async fn get_available_server(&self) -> Result<Option<ServerInfo>, redis::RedisError> {
        let mut conn = self.conn.clone();

        // HVALS récupère toutes les valeurs (les JSON) du Hash d'un coup
        let servers_json: Vec<String> = conn.hvals("servers_hash").await?;

        // Utilisation de l'approche fonctionnelle Rust pour trouver le meilleur candidat
        let best_server = servers_json
            .into_iter()
            // 1. Désérialise les JSON valides, ignore les erreurs
            .filter_map(|json| serde_json::from_str::<ServerInfo>(&json).ok())
            // 2. Ne garde que les serveurs qui ont de la place
            .filter(|server| server.players_online < server.max_players)
            // 3. (Optionnel mais recommandé) On prend le serveur le plus rempli
            // pour regrouper les joueurs et éviter d'avoir 10 serveurs avec 1 joueur.
            .max_by_key(|server| server.players_online);

        Ok(best_server)
    }
}