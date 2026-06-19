use reqwest::Client;
use std::time::Duration;
use tokio::time::sleep;

// --- CONFIGURATION ---
const GATEKEEPER_URL: &str = "http://127.0.0.1:3000";

#[tokio::main]
async fn main() {
    // Lecture des arguments (ex: cargo run -- 50 100)
    // arg 1 : Nombre de bots à créer (défaut: 10)
    // arg 2 : Délai en millisecondes entre chaque requête (défaut: 200)
    let args: Vec<String> = std::env::args().collect();
    let num_users: usize = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(10);
    let delay_ms: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(200);

    println!("🛠️ Lancement de la routine d'inscription pour {} utilisateurs...", num_users);
    println!("⏱️ Cadence : 1 requête toutes les {} ms", delay_ms);

    // Un seul client HTTP réutilisé pour profiter du pool de connexions (Keep-Alive)
    let client = Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();

    let mut success_count = 0;
    let mut error_count = 0;

    for i in 1..=num_users {
        let username = format!("user{}", i);
        let password = format!("password{}", i);

        print!("➡️ Inscription de '{}'... ", username);

        let res = client
            .post(format!("{}/register", GATEKEEPER_URL))
            .json(&serde_json::json!({ "username": username, "password": password }))
            .send()
            .await;

        match res {
            Ok(response) => {
                let status = response.status();
                if status.is_success() {
                    println!("✅ Succès");
                    success_count += 1;
                } else if status.as_u16() == 409 || status.as_u16() == 400 {
                    // Note: Adapte les codes HTTP selon la logique de ton Gatekeeper.
                    // 409 Conflict ou 400 Bad Request sont souvent utilisés si l'user existe déjà.
                    println!("⚠️ Déjà existant ou refusé (Code: {})", status);
                    success_count += 1; // On considère que le compte est prêt pour la simulation
                } else {
                    println!("❌ Échec (Code: {})", status);
                    error_count += 1;
                }
            }
            Err(e) => {
                println!("❌ Erreur réseau: {}", e);
                error_count += 1;
            }
        }

        // Le "Rate Limiter" naturel : on attend avant de faire la prochaine requête
        sleep(Duration::from_millis(delay_ms)).await;
    }

    println!("\n===========================================");
    println!("🎉 BATCH D'INSCRIPTION TERMINÉ");
    println!("✅ Comptes prêts : {}", success_count);
    println!("❌ Erreurs       : {}", error_count);
    println!("===========================================");
}