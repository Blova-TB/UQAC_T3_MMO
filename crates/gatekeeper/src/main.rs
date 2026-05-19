#[macro_use]
extern crate rocket;

mod auth;
mod jwt;
mod models;
mod routes;
mod dbGatekeeper;

use dbGatekeeper::{Database, ServerInfo};
use sqlx::{postgres::PgPoolOptions, Pool, Postgres};

use crate::routes::{get_me, login, register, get_server};

async fn create_pool(database_url: &str) -> Result<Pool<Postgres>, sqlx::Error> {
    PgPoolOptions::new()
        .max_connections(5)
        .connect(database_url)
        .await
}

#[launch]
async fn rocket() -> _ {
    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let jwt_secret = std::env::var("JWT_SECRET").expect("JWT_SECRET must be set");
    let redis_url = std::env::var("REDIS_URL").expect("REDIS_URL must be set");

    let pool = create_pool(&database_url)
        .await
        .expect("Failed to connect to Postgres");

    let database = Database::new(&redis_url)
        .await
        .expect("Failed to connect to Redis");

    println!("✅ Connecté à Redis !");

    rocket::build()
        .manage(pool)
        .manage(jwt_secret)
        .manage(database)
        .mount("/", routes![register, login, get_server, get_me])
}
