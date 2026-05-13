use axum::{
    extract::State,
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use crate::db::{Database, ServerInfo};

pub fn build_router(db: Database) -> Router {
    Router::new()
        .route("/health", get(health_check))
        .route("/servers", post(register_server))
        .route("/servers/available", get(get_available_server))
        .with_state(db)
}

async fn health_check() -> &'static str {
    "Orchestrator is running"
}

async fn register_server(
    State(db): State<Database>,
    Json(payload): Json<ServerInfo>,
) -> Result<StatusCode, (StatusCode, String)> {
    match db.save_server(&payload).await {
        Ok(_) => Ok(StatusCode::CREATED),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Erreur Redis: {}", e),
        )),
    }
}

/// Handler (GET) : Retourne un serveur disponible à la Gateway
async fn get_available_server(
    State(db): State<Database>,
) -> Result<Json<ServerInfo>, (StatusCode, String)> {
    match db.get_available_server().await {
        Ok(Some(server)) => Ok(Json(server)),

        Ok(None) => {
            // C'est ICI que l'Orchestrateur devra, plus tard, appeler Docker
            // pour lancer un nouveau conteneur.
            // En attendant, on renvoie une erreur 503 (Service Unavailable)
            Err((
                StatusCode::SERVICE_UNAVAILABLE,
                "Tous les serveurs sont pleins. Démarrage d'un nouveau serveur en cours...".to_string()
            ))
        },

        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Erreur Redis: {}", e),
        )),
    }
}