#[macro_use]
extern crate rocket;

mod auth;
mod jwt;
mod models;
mod routes;

use sqlx::{postgres::PgPoolOptions, Pool, Postgres};

use crate::routes::{get_me, login, register};

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

    let pool = create_pool(&database_url)
        .await
        .expect("Failed to connect to Postgres");

    rocket::build()
        .manage(pool)
        .manage(jwt_secret)
        .mount("/", routes![register, login, get_me])
}
