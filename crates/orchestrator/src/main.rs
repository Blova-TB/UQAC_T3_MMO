mod db;

use db::{Database, ServerInfo};

#[tokio::main]
async fn main() {
    // L'URL pointe vers "localhost" si tu lances le cargo run hors docker,
    // ou "redis" si l'orchestrateur tourne dans docker.
    let redis_url = "redis://127.0.0.1:6379/";

    println!("Connexion à Redis sur {}...", redis_url);

    let database = match Database::new(redis_url).await {
        Ok(db) => db,
        Err(e) => panic!("Erreur de connexion Redis : {}", e),
    };

    println!("✅ Connecté à Redis !");

    // Simulation d'un serveur qui vient de démarrer
    let mock_server = ServerInfo {
        container_id: "shard-01".to_string(),
        address: "127.0.0.1:5001".to_string(),
        players_online: 0,
        max_players: 100,
    };

    // Test 1 : Sauvegarde
    database.save_server(&mock_server).await.expect("Échec de la sauvegarde");
    println!("✅ Serveur mocké sauvegardé en BDD.");

    // Test 2 : Lecture
    if let Some(server) = database.get_server("shard-01").await.unwrap() {
        println!("✅ Lecture réussie : {:?}", server);
    } else {
        println!("❌ Serveur introuvable !");
    }
}