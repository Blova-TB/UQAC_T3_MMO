use bcrypt::{DEFAULT_COST, hash, verify};
use rocket::State;
use rocket::http::Status;
use rocket::serde::json::Json;
use shared::models::Status as ServerStatus;
use sqlx::{Pool, Postgres, Row, types::Uuid};

use crate::dbGatekeeper::Database;
use crate::{
    jwt::create_jwt,
    models::{AuthRequest, AuthenticatedUser, BasicCredentials},
};

#[post("/register", data = "<user_data>")]
pub async fn register(
    pool: &State<Pool<Postgres>>,
    user_data: Json<AuthRequest>,
) -> Result<&'static str, String> {
    let user_data = user_data.into_inner();
    let hashed = hash(&user_data.password, DEFAULT_COST).map_err(|_| "Erreur hachage")?;

    sqlx::query("INSERT INTO users (username, password_hash) VALUES ($1, $2)")
        .bind(&user_data.username)
        .bind(&hashed)
        .execute(&**pool)
        .await
        .map_err(|e| e.to_string())?;

    Ok("Utilisateur créé")
}

#[post("/login")]
pub async fn login(
    pool: &State<Pool<Postgres>>,
    secret: &State<String>,
    credentials: BasicCredentials,
) -> Result<String, (Status, String)> {
    let username = credentials.username;
    let password = credentials.password;

    let row = sqlx::query("SELECT id, password_hash FROM users WHERE username = $1")
        .bind(&username)
        .fetch_one(&**pool)
        .await
        .map_err(|_| (Status::Unauthorized, "Utilisateur non trouvé".to_string()))?;

    let user_id: Uuid = row
        .try_get("id")
        .map_err(|e| (Status::InternalServerError, e.to_string()))?;
    let password_hash: String = row
        .try_get("password_hash")
        .map_err(|e| (Status::InternalServerError, e.to_string()))?;
    let valid = verify(&password, &password_hash)
        .map_err(|_| (Status::Unauthorized, "Erreur vérification".to_string()))?;

    if valid {
        create_jwt(&user_id.to_string(), secret.as_str()).map_err(|_| {
            (
                Status::InternalServerError,
                "Erreur génération token".to_string(),
            )
        })
    } else {
        Err((Status::Unauthorized, "Identifiants invalides".to_string()))
    }
}

#[get("/server")]
pub async fn get_server(
    _user: AuthenticatedUser,
    db: &State<Database>
) -> Result<String, Status> {
    let servers = db.get_all_servers().await.map_err(|e| {
        eprintln!("Database error (get_all_servers): {:?}", e);
        Status::InternalServerError
    })?;

    servers
        .into_iter()
        .filter(|server| {
            server.status == ServerStatus::Online && server.players_online < (server.max_players)
        })
        .max_by_key(|server| server.players_online)
        .map(|server| server.address)
        .ok_or(Status::NotFound)
}

#[get("/me")]
pub fn get_me(user: AuthenticatedUser) -> String {
    format!("Mon ID est {}", user.user_id)
}
