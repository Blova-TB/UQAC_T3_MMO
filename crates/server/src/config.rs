// config.rs
use bevy::prelude::*;
use std::env;

#[derive(Resource)]
pub struct ServerConfig {
    pub id: String,
    pub max_players: usize,
    pub orchestrator_addr: String,
}

impl ServerConfig {
    pub fn from_env() -> Self {
        Self {
            orchestrator_addr: env::var("ORCHESTRATOR_ADDR")
                .expect("❌ ERREUR: ORCHESTRATOR_ADDR manquante."),

            id: env::var("SERVER_ID")
                .expect("❌ ERREUR: SERVER_ID manquante."),

            max_players: env::var("MAX_PLAYERS")
                .unwrap_or_else(|_| "100".to_string())
                .parse::<usize>()
                .expect("❌ ERREUR: MAX_PLAYERS doit être un entier."),
        }
    }
}