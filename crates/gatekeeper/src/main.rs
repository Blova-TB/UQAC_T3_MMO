#[macro_use] extern crate rocket;
use sqlx::postgres::PgPoolOptions;


#[get("/register")]
fn register() -> &'static str {
    "Register"
}

#[get("/login")]
fn login() -> &'static str {
    "Login"
}

#[launch]
async fn rocket() -> _ {
    let database_url = std::env::var("DATABASE_URL")
        .expect("DATABASE_URL must be set");

    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await
        .expect("Failed to connect to Postgres");

    rocket::build()
        .manage(pool)
        .mount("/", routes![register, login])
}