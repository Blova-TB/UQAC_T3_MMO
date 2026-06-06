use crate::core::{SessionData, TargetServer, TokioRuntime};
use crate::states::AppState;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use bevy::prelude::*;
use bevy::tasks::{block_on, poll_once, IoTaskPool, Task};
use bevy_egui::{egui, EguiContexts, EguiPrimaryContextPass};
use shared::web_models::Claims;
use std::time::Duration;

pub struct AuthPlugin;

impl Plugin for AuthPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AuthFormState>()
            // Phase 1 : Auth
            .add_systems(
                EguiPrimaryContextPass,
                draw_auth_ui.run_if(in_state(AppState::AuthMenu)),
            )
            .add_systems(Update, poll_http_auth.run_if(in_state(AppState::AuthMenu)))
            // Phase 2 : Fetch Server
            .add_systems(OnEnter(AppState::GatekeeperFetch), spawn_gatekeeper_request)
            .add_systems(
                Update,
                poll_gatekeeper_request.run_if(in_state(AppState::GatekeeperFetch)),
            );
    }
}

#[derive(Resource, Default)]
pub struct AuthFormState {
    pub username: String,
    pub password: String,
    pub error_message: Option<String>,
    pub is_register_mode: bool,
}

#[derive(Component)]
struct HttpAuthTask(Task<Result<(u16, String), String>>);

#[derive(Component)]
struct FetchServerTask(Task<Result<(u16, String), String>>);

fn draw_auth_ui(
    mut contexts: EguiContexts,
    mut form: ResMut<AuthFormState>,
    mut commands: Commands,
    rt: Res<TokioRuntime>,
) {
    let Ok(ctx) = contexts.ctx_mut() else { return };

    egui::Window::new("GateKeeper Authentication")
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Username:");
                ui.text_edit_singleline(&mut form.username);
            });
            ui.horizontal(|ui| {
                ui.label("Password:");
                ui.add(egui::TextEdit::singleline(&mut form.password).password(true));
            });

            if let Some(err) = &form.error_message {
                ui.colored_label(egui::Color32::RED, err);
            }

            ui.add_space(10.0);

            ui.horizontal(|ui| {
                if form.is_register_mode {
                    if ui.button("S'inscrire").clicked() {
                        spawn_auth_request(&mut commands, &rt, &form.username, &form.password, true);
                    }
                    if ui.button("Aller à la connexion").clicked() {
                        form.is_register_mode = false;
                        form.error_message = None;
                    }
                } else {
                    if ui.button("Se connecter").clicked() {
                        spawn_auth_request(&mut commands, &rt, &form.username, &form.password, false);
                    }
                    if ui.button("Créer un compte").clicked() {
                        form.is_register_mode = true;
                        form.error_message = None;
                    }
                }
            });
        });
}

fn spawn_auth_request(
    commands: &mut Commands,
    rt: &TokioRuntime,
    user: &str,
    pass: &str,
    is_register: bool,
) {
    let username = user.to_string();
    let password = pass.to_string();

    println!(
        ">>> DEBUG: Lancement de la requête {} pour l'utilisateur '{}'",
        if is_register { "d'inscription" } else { "de connexion" },
        username
    );

    let join_handle = rt.0.spawn(async move {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .map_err(|e| e.to_string())?;

        let res = if is_register {
            client
                .post("http://127.0.0.1:3000/register")
                .json(&serde_json::json!({ "username": username, "password": password }))
                .send()
                .await
        } else {
            client
                .post("http://127.0.0.1:3000/login")
                .basic_auth(username, Some(password))
                .send()
                .await
        };

        match res {
            Ok(response) => {
                let status = response.status().as_u16();
                let text = response.text().await.unwrap_or_default();
                Ok((status, text))
            }
            Err(e) => Err(e.to_string()),
        }
    });

    let task = IoTaskPool::get().spawn(async move {
        join_handle.await.unwrap_or_else(|e| Err(format!("Thread panic: {}", e)))
    });

    commands.spawn(HttpAuthTask(task));
}

fn poll_http_auth(
    mut commands: Commands,
    mut q_task: Query<(Entity, &mut HttpAuthTask)>,
    mut form: ResMut<AuthFormState>,
    mut next_state: ResMut<NextState<AppState>>,
) {
    for (entity, mut task) in &mut q_task {
        if let Some(result) = block_on(poll_once(&mut task.0)) {
            commands.entity(entity).despawn();

            match result {
                Ok((status, body)) => {
                    if status == 200 {
                        if form.is_register_mode {
                            form.error_message = Some("Inscription réussie. Connectez-vous.".into());
                            form.is_register_mode = false;
                        } else {
                            match extract_unverified_claims(&body) {
                                Some(claims) => {
                                    commands.insert_resource(SessionData {
                                        gatekeeper_token: body,
                                        session_token: String::new(),
                                        custom_id: claims.custom_id,
                                    });
                                    next_state.set(AppState::GatekeeperFetch);
                                }
                                None => form.error_message = Some("Token JWT invalide ou corrompu".into()),
                            }
                        }
                    } else {
                        form.error_message = Some(format!("Erreur {} : {}", status, body));
                    }
                }
                Err(e) => form.error_message = Some(format!("Erreur réseau: {}", e)),
            }
        }
    }
}

fn extract_unverified_claims(token: &str) -> Option<Claims> {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return None;
    }
    let payload = URL_SAFE_NO_PAD.decode(parts[1]).ok()?;
    serde_json::from_slice(&payload).ok()
}

fn spawn_gatekeeper_request(
    mut commands: Commands,
    session: Res<SessionData>,
    rt: Res<TokioRuntime>,
) {
    let jwt_token = session.gatekeeper_token.clone();

    let join_handle = rt.0.spawn(async move {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .map_err(|e| e.to_string())?;
        match client
            .get("http://127.0.0.1:3000/server")
            .bearer_auth(jwt_token)
            .send()
            .await
        {
            Ok(res) => {
                let status = res.status().as_u16();
                let text = res.text().await.unwrap_or_default();
                Ok((status, text))
            }
            Err(e) => Err(e.to_string()),
        }
    });

    let task = IoTaskPool::get().spawn(async move {
        join_handle.await.unwrap_or_else(|e| Err(format!("Thread panic: {}", e)))
    });
    commands.spawn(FetchServerTask(task));
}

fn poll_gatekeeper_request(
    mut commands: Commands,
    mut q_task: Query<(Entity, &mut FetchServerTask)>,
    mut session: ResMut<SessionData>,
    mut next_state: ResMut<NextState<AppState>>,
) {
    for (entity, mut task) in &mut q_task {
        if let Some(result) = block_on(poll_once(&mut task.0)) {
            commands.entity(entity).despawn();

            match result {
                Ok((200, response_text)) => {
                    match serde_json::from_str::<serde_json::Value>(&response_text) {
                        Ok(json) => {
                            let broker_addr = json["broker_addr"].as_str().unwrap_or("");
                            session.session_token = json["session_token"].as_str().unwrap_or("").to_string();

                            let parts: Vec<&str> = broker_addr.split(':').collect();
                            if parts.len() == 2 && !session.session_token.is_empty() {
                                let mut target_ip = parts[0].to_string();

                                if target_ip.starts_with("172.") || target_ip.starts_with("10.") {
                                    target_ip = "127.0.0.1".to_string();
                                }

                                if let Ok(port) = parts[1].parse::<u16>() {
                                    commands.insert_resource(TargetServer { ip: target_ip, port });
                                    next_state.set(AppState::Connecting);
                                }
                            }
                        }
                        Err(_) => eprintln!("Erreur lors de la lecture du JSON du Gatekeeper."),
                    }
                }
                Ok((status, err)) => eprintln!("Erreur Gatekeeper: {} - {}", status, err),
                Err(e) => eprintln!("Erreur réseau Fetch: {}", e),
            }
        }
    }
}