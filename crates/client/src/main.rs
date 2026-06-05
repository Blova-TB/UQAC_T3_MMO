use bevy::prelude::*;
use bevy::tasks::{block_on, poll_once, IoTaskPool, Task};
use bevy_egui::{egui, EguiContexts, EguiPlugin, EguiPrimaryContextPass};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use tokio::runtime::Runtime;
use std::collections::HashMap;
use std::time::Duration;

// --- Imports de votre crate 'shared' ---
use shared::network::{GameConnection, GameNetworkEvent, GamePeer, GameStreamReliability};
use shared::network::protocols::QuicBackend;
use shared::models::{BrokerHandshakeClient, ServerSyncMessage, PlayerData, ServerBinaryPacket};
use shared::web_models::Claims;

// --- Ressources ---

#[derive(Resource)]
pub struct TokioRuntime(pub Runtime);

impl Default for TokioRuntime {
    fn default() -> Self {
        Self(
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("Échec de l'initialisation du Runtime Tokio"),
        )
    }
}

#[derive(States, Default, Debug, Clone, Eq, PartialEq, Hash)]
pub enum AppState {
    #[default]
    AuthMenu,        // Interface Egui Login/Register
    GatekeeperFetch, // Requête GET /server
    Connecting,      // Handshake QUIC
    InGame,          // Boucle de jeu ECS
}

#[derive(Resource, Default)]
pub struct AuthFormState {
    pub username: String,
    pub password: String,
    pub error_message: Option<String>,
    pub is_register_mode: bool,
}

#[derive(Resource)]
pub struct SessionData {
    pub gatekeeper_token: String,
    pub session_token: String,
    pub custom_id: u32,
}

#[derive(Resource)]
pub struct TargetServer {
    pub ip: String,
    pub port: u16,
}

#[derive(Resource)]
pub struct ClientState {
    pub peer: GamePeer,
    pub connection: Option<GameConnection>,
}

#[derive(Resource)]
pub struct GameAssets {
    pub player_sprite: Handle<Image>,
}

// --- Composants ---

#[derive(Component)]
pub struct NetworkEntity(pub u32);

#[derive(Component)]
pub struct LocalPlayer;

#[derive(Component)]
struct HttpAuthTask(Task<Result<(u16, String), String>>);

#[derive(Component)]
struct FetchServerTask(Task<Result<(u16, String), String>>);

// --- Events ---

#[derive(Message)]
pub struct NetworkSnapshotEvent {
    pub players: Vec<PlayerData>,
}

// --- Point d'entrée ---

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "MMO Client".into(),
                resolution: bevy::window::WindowResolution::new(1280, 720),
                present_mode: bevy::window::PresentMode::AutoNoVsync,
                ..default()
            }),
            ..default()
        }))
        .add_plugins(EguiPlugin::default())
        .init_state::<AppState>()
        .init_resource::<TokioRuntime>()
        .init_resource::<AuthFormState>()
        .add_message::<NetworkSnapshotEvent>()

        .add_systems(Startup, setup_core)


        // Phase 1 : Auth (UI Egui et Réseau)
        // 🚀 L'interface graphique va dans le pass spécifique d'Egui
        .add_systems(EguiPrimaryContextPass, draw_auth_ui.run_if(in_state(AppState::AuthMenu)))

        // La logique HTTP reste dans l'Update global de Bevy
        .add_systems(Update, poll_http_auth.run_if(in_state(AppState::AuthMenu)))

        // Phase 2 : Fetch Server
        .add_systems(OnEnter(AppState::GatekeeperFetch), spawn_gatekeeper_request)
        .add_systems(Update, poll_gatekeeper_request.run_if(in_state(AppState::GatekeeperFetch)))

        // Phase 3 : Connexion QUIC & Handshake
        .add_systems(OnEnter(AppState::Connecting), init_game_connection)
        .add_systems(Update, handle_connection_handshake.run_if(in_state(AppState::Connecting)))

        // Phase 4 : In Game (Réseau + Synchro ECS)
        .add_systems(Update, (handle_ingame_network, sync_players_state).run_if(in_state(AppState::InGame)))

        .run();
}

// --- Systèmes d'Initialisation ---

fn setup_core(mut commands: Commands, asset_server: Res<AssetServer>) {
    commands.spawn(Camera2d);
    commands.insert_resource(GameAssets {
        player_sprite: asset_server.load("circle.png"), // Assurez-vous que cet asset existe dans assets/
    });
}

// --- Phase 1 : Interface d'Authentification ---

fn draw_auth_ui(
    mut contexts: EguiContexts,
    mut form: ResMut<AuthFormState>,
    mut commands: Commands,
    rt: Res<TokioRuntime>,
) {
    let Ok(ctx) = contexts.ctx_mut() else { return; };

    egui::Window::new("GateKeeper Authentication")
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| { // <-- Utilisez l'instance sécurisée `ctx` ici
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

    let join_handle = rt.0.spawn(async move {
        let client = reqwest::Client::builder().timeout(Duration::from_secs(5)).build().map_err(|e| e.to_string())?;

        let res = if is_register {
            client.post("http://127.0.0.1:3000/register")
                .json(&serde_json::json!({ "username": username, "password": password }))
                .send().await
        } else {
            client.post("http://127.0.0.1:3000/login")
                .basic_auth(username, Some(password))
                .send().await
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
    if parts.len() != 3 { return None; }
    let payload = URL_SAFE_NO_PAD.decode(parts[1]).ok()?;
    serde_json::from_slice(&payload).ok()
}

// --- Phase 2 : Gatekeeper Fetch ---

fn spawn_gatekeeper_request(
    mut commands: Commands,
    session: Res<SessionData>,
    rt: Res<TokioRuntime>
) {
    let jwt_token = session.gatekeeper_token.clone();

    let join_handle = rt.0.spawn(async move {
        let client = reqwest::Client::builder().timeout(Duration::from_secs(5)).build().map_err(|e| e.to_string())?;
        match client.get("http://127.0.0.1:3000/server").bearer_auth(jwt_token).send().await {
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
                            if parts.len() == 2 && session.session_token.len() > 0 {
                                if let Ok(port) = parts[1].parse::<u16>() {
                                    commands.insert_resource(TargetServer {
                                        ip: parts[0].to_string(),
                                        port,
                                    });
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

// --- Phase 3 : Connexion QUIC ---

fn init_game_connection(mut commands: Commands, target: Res<TargetServer>) {
    let backend = QuicBackend::new();
    let peer = GamePeer::new(backend);

    peer.connect(&target.ip, target.port).expect("Échec de l'initialisation de la connexion QUIC");

    commands.insert_resource(ClientState {
        peer,
        connection: None,
    });
}

fn handle_connection_handshake(
    mut state: ResMut<ClientState>,
    session: Res<SessionData>,
    mut next_state: ResMut<NextState<AppState>>,
    mut exit: MessageWriter<AppExit>,
) {
    while let Ok(Some(event)) = state.peer.poll() {
        match event {
            GameNetworkEvent::Connected(conn) => {
                state.connection = Some(conn.clone());
                let _ = state.peer.create_stream(conn, GameStreamReliability::Reliable);
            }
            GameNetworkEvent::StreamCreated(conn, stream) => {
                if stream.is_reliable() {
                    let handshake = BrokerHandshakeClient {
                        jwt_token: session.session_token.as_bytes().to_vec(),
                    };

                    if state.peer.send(&conn, &stream, handshake.to_bytes()).is_ok() {
                        next_state.set(AppState::InGame);
                    } else {
                        exit.write(AppExit::Error(std::num::NonZeroU8::new(1).unwrap()));
                    }
                }
            }
            GameNetworkEvent::Disconnected(_) => {
                exit.write(AppExit::Success);
            }
            _ => {}
        }
    }
}

// --- Phase 4 : Boucle en Jeu ---

fn handle_ingame_network(
    mut state: ResMut<ClientState>,
    mut ev_snapshot: MessageWriter<NetworkSnapshotEvent>,
    mut exit: MessageWriter<AppExit>,
) {
    while let Ok(Some(event)) = state.peer.poll() {
        match event {
            GameNetworkEvent::Message { data, .. } => {
                // Utilisation du trait défini dans shared::models
                use shared::models::ServerBinaryPacket;

                if let Some(msg) = ServerSyncMessage::try_from_bytes(data) {
                    ev_snapshot.write(NetworkSnapshotEvent { players: msg.players });
                }
            }
            GameNetworkEvent::Disconnected(_) => {
                exit.write(AppExit::Success);
            }
            _ => {}
        }
    }
}

fn sync_players_state(
    mut commands: Commands,
    mut events: MessageReader<NetworkSnapshotEvent>,
    mut q_players: Query<(Entity, &NetworkEntity, &mut Transform)>,
    assets: Res<GameAssets>,
    session: Res<SessionData>,
) {
    for event in events.read() {
        let mut existing_entities: HashMap<u32, (Entity, Mut<Transform>)> = q_players
            .iter_mut()
            .map(|(e, net_id, transform)| (net_id.0, (e, transform)))
            .collect();

        for server_player in &event.players {
            let pid = server_player.client_id.as_u32();

            if let Some((_, transform)) = existing_entities.get_mut(&pid) {
                // Interpolation recommandée pour le futur. Set direct pour l'instant.
                transform.translation.x = server_player.pos.x;
                transform.translation.y = server_player.pos.y;

                existing_entities.remove(&pid);
            } else {
                let mut entity_cmds = commands.spawn((
                    Sprite {
                        image: assets.player_sprite.clone(),
                        color: if pid == session.custom_id {
                            Color::srgb(0.0, 1.0, 0.0)
                        } else {
                            Color::srgb(1.0, 0.0, 0.0)
                        },
                        custom_size: Some(Vec2::new(32.0, 32.0)),
                        ..default()
                    },
                    Transform::from_xyz(server_player.pos.x, server_player.pos.y, 0.0),
                    NetworkEntity(pid),
                ));

                if pid == session.custom_id {
                    entity_cmds.insert(LocalPlayer);
                }
            }
        }

        // Nettoyage strict des entités absentes de la zone de réplication (O(N) despawn map)
        for (entity, _) in existing_entities.values() {
            commands.entity(*entity).despawn();
        }
    }
}