mod db;
mod api;

use db::{Database, ServerInfo};
use std::net::SocketAddr;

#[tokio::main]
async fn main() {
    // 1. Récupération de l'URL via variable d'environnement (avec fallback pour le local)
    let redis_url = std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379/".to_string());

    println!("Connexion à Redis sur {}...", redis_url);

    let database = match Database::new(&redis_url).await {
        Ok(db) => db,
        Err(e) => panic!("Erreur de connexion Redis : {}", e),
    };

    println!("✅ Connecté à Redis !");

    // 2. Mock initial (pour tes tests)
    let mock_server = ServerInfo {
        container_id: "shard-01".to_string(),
        address: "127.0.0.1:5001".to_string(),
        players_online: 0,
        max_players: 100,
    };

    database.save_server(&mock_server).await.expect("Échec de la sauvegarde");
    println!("✅ Serveur mocké sauvegardé en BDD.");

    // 3. Lancement de l'API Axum (ceci bloque le thread et maintient le conteneur en vie)
    let app = api::build_router(database);
    let addr = SocketAddr::from(([0, 0, 0, 0], 4000));

    println!("🚀 API Orchestrateur en écoute sur {}", addr);
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();

    if let Err(e) = axum::serve(listener, app).await {
        eprintln!("Erreur du serveur HTTP : {}", e);
    }
}