use bcrypt::{DEFAULT_COST, hash, verify};
use rocket::State;
use rocket::serde::json::Json;
use sqlx::{Pool, Postgres, Row, types::Uuid};

use crate::{auth::AuthenticatedUser, jwt::create_jwt, models::AuthRequest};

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

#[post("/login", data = "<user_data>")]
pub async fn login(
    pool: &State<Pool<Postgres>>,
    secret: &State<String>,
    user_data: Json<AuthRequest>,
) -> Result<String, String> {
    let user_data = user_data.into_inner();

    let row = sqlx::query("SELECT id, password_hash FROM users WHERE username = $1")
        .bind(&user_data.username)
        .fetch_one(&**pool)
        .await
        .map_err(|_| "Utilisateur non trouvé")?;

    let user_id: Uuid = row.try_get("id").map_err(|e| e.to_string())?;
    let password_hash: String = row.try_get("password_hash").map_err(|e| e.to_string())?;
    let valid = verify(&user_data.password, &password_hash).map_err(|_| "Erreur vérification")?;

    if valid {
        create_jwt(&user_id.to_string(), secret.as_str())
            .map_err(|_| "Erreur génération token".to_string())
    } else {
        Err("Identifiants invalides".to_string())
    }
}

#[get("/me")]
pub fn get_me(user: AuthenticatedUser) -> String {
    format!("Mon ID est {}", user.user_id)
}
