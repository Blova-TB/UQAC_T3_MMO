#[macro_use]
extern crate rocket;
use tokio::net::lookup_host;
use sqlx::postgres::PgPoolOptions;

mod routes;

use crate::routes::{get_me, login, register, get_server};

pub struct BrokerConfig {
    pub resolved_address: String,
}

#[rocket::main]
async fn main() -> Result<(), rocket::Error> {
    println!("🔍 Résolution de l'adresse du Broker...");

    let broker_socket = lookup_host("broker:5000")
        .await
        .expect("❌ Impossible de contacter le DNS Docker pour le Broker")
        .next()
        .expect("❌ Le DNS a répondu, mais aucune IP n'a été trouvée pour 'broker'");

    let broker_address = format!("{}:5000", broker_socket.ip());

    println!("✅ IP du Broker trouvée et mise en cache : {}", broker_address);

    let broker_config = BrokerConfig {
        resolved_address: broker_address,
    };

    let jwt_secret = std::env::var("JWT_SECRET").expect("JWT_SECRET must be set");

    let db_url = std::env::var("DATABASE_URL").expect("❌ DATABASE_URL must be set");
    println!("🔌 Connexion à la base de données PostgreSQL...");

    let db_pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&db_url)
        .await
        .expect("❌ Impossible de se connecter à la base de données");

    println!("✅ Connecté à PostgreSQL !");

    let _rocket = rocket::build()
        .manage(broker_config)
        .manage(jwt_secret)
        .manage(db_pool)
        .mount("/", routes![register, login, get_server, get_me])
        .launch()
        .await?;

    Ok(())
}
