use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,
    pub custom_id: u32,
    pub pos_x: f32,
    pub pos_y: f32,
    pub exp: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AuthenticatedUser {
    pub user_id: String,
    pub custom_id: u32,
    pub pos_x: f32,
    pub pos_y: f32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BasicCredentials {
    pub username: String,
    pub password: String,
}

#[derive(Deserialize)]
pub struct AuthRequest {
    pub username: String,
    pub password: String,
}

// Nouvelle structure pour renvoyer proprement l'accès au Broker
#[derive(Serialize)]
pub struct ServerResponse {
    pub broker_addr: String,
    pub session_token: String,
}