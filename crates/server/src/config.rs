// config.rs
use bevy::prelude::*;
use std::env;

#[derive(Resource)]
pub struct ServerConfig {
    pub id: String,
    pub max_players: usize,
    pub broker_addr: String,
    pub orchestrator_addr: String,
}

impl ServerConfig {
    pub fn from_env() -> Self {
        Self {
            orchestrator_addr: env::var("ORCHESTRATOR_ADDR")
                .expect("❌ ERREUR: ORCHESTRATOR_ADDR manquante."),

            id: env::var("SERVER_ID")
                .expect("❌ ERREUR: SERVER_ID manquante."),

            max_players: env::var("SERVER_MAX_PLAYERS")
                .unwrap_or_else(|_| "100".to_string())
                .parse::<usize>()
                .expect("❌ ERREUR: MAX_PLAYERS doit être un entier."),
            broker_addr: env::var("BROKER_ADDR").unwrap_or_else(|_| "127.0.0.1:5000".to_string()),
        }
    }
}