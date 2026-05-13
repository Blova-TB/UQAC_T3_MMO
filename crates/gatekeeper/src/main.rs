#[macro_use] extern crate rocket;

#[get("/health")]
fn health_check() -> &'static str {
    "OK"
}

#[get("/servers")]
fn list_servers() -> &'static str {
    "Liste des serveurs de jeu"
}

#[launch]
fn rocket() -> _ {
    rocket::build()
        .mount("/", routes![health_check, list_servers])
}