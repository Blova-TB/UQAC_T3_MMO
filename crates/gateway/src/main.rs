use axum::{routing::get, Router};
use std::net::SocketAddr;

#[tokio::main]
async fn main() {
    let app = Router::new()
        .route("/health", get(health_check))
        .route("/servers", get(list_servers));

    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    println!("Gateway en écoute sur {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn health_check() -> &'static str {
    "OK"
}

async fn list_servers() -> &'static str {
    "Liste des serveurs de jeu"
}
