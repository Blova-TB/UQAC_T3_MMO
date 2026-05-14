mod db;
mod docker;

use db::{Database, ServerInfo};
use docker::DockerOrchestrator;

#[tokio::main]
async fn main() {
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

    let orchestrator = match DockerOrchestrator::new() {
        Ok(orch) => {
            println!("✅ Connecté au daemon Docker !");
            orch
        },
        Err(e) => {
            eprintln!("❌ Erreur fatale : Impossible de se connecter à Docker. Détails : {}", e);
            eprintln!("👉 Assure-toi que Docker Desktop (ou le daemon) tourne sur ta machine.");
            return;
        }
    };

    println!("⏳ Tentative de lancement d'un conteneur de test (Alpine)...");
    
    match orchestrator.test_spawn().await {
        Ok(id) => {
            println!("✅ Succès total ! Le conteneur tourne.");
            println!("🔑 ID du conteneur : {}", id);
            println!("👉 Ouvre un autre terminal et tape : docker ps");
            println!("(Le conteneur va s'éteindre et se supprimer tout seul dans 60 secondes)");
        },
        Err(e) => {
            eprintln!("❌ Échec lors de la création ou du démarrage du conteneur.");
            eprintln!("Détails de l'erreur : {}", e);
            eprintln!("👉 Astuce : As-tu pensé à faire un 'docker pull alpine' avant ?");
        }
    }
}