use bcrypt::{DEFAULT_COST, hash, verify};
use rand::Rng;
use rocket::State;
use rocket::http::Status;
use rocket::serde::json::Json;
use sqlx::{Pool, Postgres, Row, types::Uuid};

use crate::{
    jwt::create_jwt,
    models::{AuthRequest, AuthenticatedUser, BasicCredentials, ServerResponse},
};

#[post("/register", data = "<user_data>")]
pub async fn register(
    pool: &State<Pool<Postgres>>,
    user_data: Json<AuthRequest>,
) -> Result<&'static str, String> {
    let user_data = user_data.into_inner();
    let hashed = hash(&user_data.password, DEFAULT_COST).map_err(|_| "Erreur hachage")?;
    const MAX_RETRIES: u32 = 5;

    for _ in 0..MAX_RETRIES {
        let random_client_id: i32 = rand::thread_rng().gen_range(1..=0x0FFF_FFFF);

        let result = sqlx::query("INSERT INTO users (username, password_hash, custom_id, pos_x, pos_y) VALUES ($1, $2, $3, $4, $5)")
            .bind(&user_data.username)
            .bind(&hashed)
            .bind(random_client_id)
            .bind(0.0f32)
            .bind(0.0f32)
            .execute(&**pool)
            .await;

        match result {
            Ok(_) => return Ok("Utilisateur créé"),

            Err(sqlx::Error::Database(db_err)) => {
                if db_err.is_unique_violation() {
                    if let Some(constraint) = db_err.constraint() {
                        if constraint.contains("username") {
                            return Err("Ce nom d'utilisateur est déjà pris.".to_string());
                        }

                        if constraint.contains("custom_id") {
                            println!("⚠️ Collision d'ID détectée pour {}, nouvelle tentative...", random_client_id);
                            continue;
                        }
                    }
                }
                return Err(db_err.message().to_string());
            }
            Err(e) => return Err(e.to_string()),
        }
    }
    Err("Impossible de générer un ID unique après plusieurs tentatives. Veuillez réessayer.".to_string())
}

#[post("/login")]
pub async fn login(
    pool: &State<Pool<Postgres>>,
    secret: &State<String>,
    credentials: BasicCredentials,
) -> Result<String, (Status, String)> {
    let username = credentials.username;
    let password = credentials.password;

    // On lit tout !
    let row = sqlx::query("SELECT id, password_hash, custom_id, pos_x, pos_y FROM users WHERE username = $1")
        .bind(&username)
        .fetch_one(&**pool)
        .await
        .map_err(|_| (Status::Unauthorized, "Utilisateur non trouvé".to_string()))?;

    let user_id: Uuid = row.try_get("id").map_err(|e| (Status::InternalServerError, e.to_string()))?;
    let password_hash: String = row.try_get("password_hash").map_err(|e| (Status::InternalServerError, e.to_string()))?;

    let custom_id_i32: i32 = row.try_get("custom_id").map_err(|e| (Status::InternalServerError, e.to_string()))?;

    let pos_x: f32 = row.try_get("pos_x").unwrap_or(0.0);
    let pos_y: f32 = row.try_get("pos_y").unwrap_or(0.0);

    let valid = verify(&password, &password_hash)
        .map_err(|_| (Status::Unauthorized, "Erreur vérification".to_string()))?;

    if valid {
        // Token d'API Web valide 24 heures
        create_jwt(&user_id.to_string(), custom_id_i32 as u32, pos_x, pos_y, secret.as_str(), 24).map_err(|_| {
            (Status::InternalServerError, "Erreur génération token".to_string())
        })
    } else {
        Err((Status::Unauthorized, "Identifiants invalides".to_string()))
    }
}

#[rocket::get("/server")]
pub async fn get_server(
    broker_config: &State<crate::BrokerConfig>,
    secret: &State<String>,
    user: AuthenticatedUser
) -> Result<Json<ServerResponse>, Status> {
    let session_token = create_jwt(&user.user_id, user.custom_id, user.pos_x, user.pos_y, secret.as_str(), 1)
        .map_err(|_| Status::InternalServerError)?;

    Ok(Json(ServerResponse {
        broker_addr: broker_config.resolved_address.clone(),
        session_token,
    }))
}

#[get("/me")]
pub fn get_me(user: AuthenticatedUser) -> String {
    format!("Mon ID est {}", user.user_id)
}
