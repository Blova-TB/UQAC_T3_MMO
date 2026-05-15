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

    let orchestrator = DockerOrchestrator::new().expect("❌ Échec Docker");

    let image_name = "uqac_t3_mmo-server:local";
    let container_name = "game-shard-01";
    let external_port = "4001";

    let orchestrator_internal_url = "quic://orchestrator:4000";

    println!("⏳ Lancement de l'instance '{}'...", container_name);

    match orchestrator.spawn_game_server(container_name, image_name, orchestrator_internal_url, external_port).await {
        Ok(id) => {
            let short_id = &id[..12];
            println!("✅ Serveur en ligne ! Container ID : {}", short_id);

            let server_info = ServerInfo {
                container_id: short_id.to_string(),
                address: format!("127.0.0.1:{}", external_port),
                players_online: 0,
                max_players: 100,
            };

            database.save_server(&server_info).await.expect("❌ Erreur Redis");
            println!("💾 Instance '{}' enregistrée en BDD.", container_name);
        },
        Err(e) => {
            eprintln!("❌ Échec : {}", e);
        }
    }
}