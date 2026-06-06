use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use bevy::prelude::*;
use bevy::tasks::{IoTaskPool, Task, block_on, poll_once};
use bevy_egui::{EguiContexts, EguiPlugin, EguiPrimaryContextPass, egui};
use shared::game_protocol::{CustomId, INPUT_ACTION, INPUT_DOWN, INPUT_LEFT, INPUT_RIGHT, INPUT_UP, LogicalStream, PlayerData, PlayerInput, PlayerInputPayload, GameMessage};
use shared::models::{Broadcast, BrokerHandshakeClient, Publish, RefuseClient, ServerBinaryPacket};
use shared::network::protocols::QuicBackend;
use shared::network::{
    GameConnection, GameNetworkEvent, GamePeer, GameStream, GameStreamReliability,
};
use shared::web_models::Claims;
use std::collections::HashMap;
use std::time::Duration;
use tokio::runtime::Runtime;

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
    AuthMenu, // Interface Egui Login/Register
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

#[derive(Resource)]
pub struct PlayerInputBuffer {
    /// Index 0 = Frame actuelle. Index 15 = Frame la plus ancienne.
    pub history: [u8; 16],
}

impl Default for PlayerInputBuffer {
    fn default() -> Self {
        Self { history: [0; 16] }
    }
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
        .init_resource::<PlayerInputBuffer>()
        .insert_resource(Time::<Fixed>::from_hz(60.0))
        .add_message::<NetworkSnapshotEvent>()
        .add_systems(Startup, setup_core)
        // Phase 1 : Auth (UI Egui et Réseau)
        // 🚀 L'interface graphique va dans le pass spécifique d'Egui
        .add_systems(
            EguiPrimaryContextPass,
            draw_auth_ui.run_if(in_state(AppState::AuthMenu)),
        )
        // La logique HTTP reste dans l'Update global de Bevy
        .add_systems(Update, poll_http_auth.run_if(in_state(AppState::AuthMenu)))
        // Phase 2 : Fetch Server
        .add_systems(OnEnter(AppState::GatekeeperFetch), spawn_gatekeeper_request)
        .add_systems(
            Update,
            poll_gatekeeper_request.run_if(in_state(AppState::GatekeeperFetch)),
        )
        // Phase 3 : Connexion QUIC & Handshake
        .add_systems(OnEnter(AppState::Connecting), init_game_connection)
        .add_systems(
            Update,
            handle_connection_handshake.run_if(in_state(AppState::Connecting)),
        )
        // Phase 4 : In Game (Réseau + Synchro ECS)
        .add_systems(
            Update,
            (handle_ingame_network, sync_players_state).run_if(in_state(AppState::InGame)),
        )
        .add_systems(
            FixedUpdate,
            (gather_and_store_inputs, broadcast_player_inputs)
                .chain()
                .run_if(in_state(AppState::InGame)),
        )
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
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };

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
                        spawn_auth_request(
                            &mut commands,
                            &rt,
                            &form.username,
                            &form.password,
                            true,
                        );
                    }
                    if ui.button("Aller à la connexion").clicked() {
                        form.is_register_mode = false;
                        form.error_message = None;
                    }
                } else {
                    if ui.button("Se connecter").clicked() {
                        spawn_auth_request(
                            &mut commands,
                            &rt,
                            &form.username,
                            &form.password,
                            false,
                        );
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

    //todo debug
    println!(
        ">>> DEBUG:Lancement de la requête {} pour l'utilisateur '{}' avec le password {}",
        if is_register {
            "d'inscription"
        } else {
            "de connexion"
        },
        username,
        password
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
                //todo debug
                println!(">>> DEBUG:Réponse du serveur ({}): {}", status, text); //todo debug
                Ok((status, text))
            }
            Err(e) => {
                //todo debug
                println!(">>> DEBUG:Erreur lors de la requête HTTP: {}", e); //todo debug
                Err(e.to_string())
            }
        }
    });

    //todo debug
    println!(">>> DEBUG:result du spawn_auth_request: {:?}", join_handle);

    let task = IoTaskPool::get().spawn(async move {
        join_handle
            .await
            .unwrap_or_else(|e| Err(format!("Thread panic: {}", e)))
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
                            form.error_message =
                                Some("Inscription réussie. Connectez-vous.".into());
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
                                None => {
                                    form.error_message =
                                        Some("Token JWT invalide ou corrompu".into())
                                }
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

// --- Phase 2 : Gatekeeper Fetch ---

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
        join_handle
            .await
            .unwrap_or_else(|e| Err(format!("Thread panic: {}", e)))
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
                            session.session_token =
                                json["session_token"].as_str().unwrap_or("").to_string();

                            let parts: Vec<&str> = broker_addr.split(':').collect();
                            if parts.len() == 2 && session.session_token.len() > 0 {
                                let mut target_ip = parts[0].to_string();

                                //todo debug
                                if target_ip.starts_with("172.") || target_ip.starts_with("10.") {
                                    println!(
                                        ">>> DEBUG: IP interne Docker détectée ({}), forçage vers 127.0.0.1",
                                        target_ip
                                    );
                                    target_ip = "127.0.0.1".to_string();
                                }

                                if let Ok(port) = parts[1].parse::<u16>() {
                                    //todo debug
                                    println!(
                                        ">>> DEBUG: Cible finale du Client -> {}:{}",
                                        target_ip, port
                                    );
                                    commands.insert_resource(TargetServer {
                                        ip: target_ip,
                                        port,
                                    });
                                    next_state.set(AppState::Connecting);
                                    //todo debug
                                    println!(">>> DEBUG: Connecting ...");
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

    println!(
        ">>> DEBUG: Tentative de connexion QUIC (UDP) vers {}:{}",
        target.ip, target.port
    );

    match peer.connect(&target.ip, target.port) {
        Ok(_) => println!(">>> DEBUG: socket QUIC ouvert. En attente du Handshake"),
        Err(e) => eprintln!(
            ">>> ERREUR: Échec immédiat de la création du socket QUIC: {:?}",
            e
        ),
    }

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
                let _ = state
                    .peer
                    .create_stream(conn, GameStreamReliability::Reliable);
            }
            GameNetworkEvent::StreamCreated(conn, stream) => {
                if stream.is_reliable() {
                    let handshake = BrokerHandshakeClient {
                        jwt_token: session.session_token.as_bytes().to_vec(),
                    };

                    if state
                        .peer
                        .send(&conn, &stream, handshake.to_bytes())
                        .is_ok()
                    {
                        next_state.set(AppState::InGame);
                        //todo debug
                        println!(">>> DEBUG: Handshake envoyé avec succès, passage en InGame");
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
    mut next_state: ResMut<NextState<AppState>>,
) {
    while let Ok(Some(event)) = state.peer.poll() {
        match event {
            GameNetworkEvent::Message {stream, data, .. } => {
                if data.is_empty() { return; }
                let tag = data[0];

                match tag {
                    Broadcast::TAG => {
                        let Some(pkt) = Broadcast::try_from_bytes(data) else { return; };
                        if let Some(game_message) = GameMessage::decode(stream.stream_id, &pkt.payload) {
                            match game_message {
                                GameMessage::WorldSync(sync_data) => {
                                    ev_snapshot.write(NetworkSnapshotEvent {
                                        players: sync_data.entities,
                                    });
                                }
                                _ => {}
                            }
                        } else {
                            eprintln!("⚠️ Impossible de décoder le payload métier sur le stream : {}", stream.stream_id);
                        }
                    }
                    RefuseClient::TAG => {
                        eprintln!("❌ Le Broker a refusé la connexion du client.");

                        // on retourne à l'écran d'authentification pour laisser l'utilisateur tenter une nouvelle connexion
                        next_state.set(AppState::AuthMenu);
                    }
                    _ => {}
                }
            }
            GameNetworkEvent::Disconnected(_) => {
                println!("Client : Session fermée par le Broker.");
                next_state.set(AppState::AuthMenu);
            }
            GameNetworkEvent::Error { inner, .. } => {
                eprintln!("Client : Perte de paquets ou erreur d'exécution réseau : {:?}", inner);
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
                transform.translation.x = server_player.pos.0;
                transform.translation.y = server_player.pos.1;

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
                    Transform::from_xyz(server_player.pos.0, server_player.pos.1, 0.0),
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

fn gather_and_store_inputs(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut input_buffer: ResMut<PlayerInputBuffer>,
) {
    let mut current_frame_input = 0u8;

    if keyboard.pressed(KeyCode::KeyW) || keyboard.pressed(KeyCode::KeyZ) {
        current_frame_input |= INPUT_UP;
    }
    if keyboard.pressed(KeyCode::KeyS) {
        current_frame_input |= INPUT_DOWN;
    }
    if keyboard.pressed(KeyCode::KeyA) || keyboard.pressed(KeyCode::KeyQ){
        current_frame_input |= INPUT_LEFT;
    }
    if keyboard.pressed(KeyCode::KeyD) {
        current_frame_input |= INPUT_RIGHT;
    }
    if keyboard.pressed(KeyCode::KeyE) {
        current_frame_input |= INPUT_ACTION;
    }

    // pas opti mais tant pis
    input_buffer.history.copy_within(0..15, 1);

    input_buffer.history[0] = current_frame_input;
}

fn broadcast_player_inputs(
    input_buffer: Res<PlayerInputBuffer>,
    state: Res<ClientState>,
    session: Res<SessionData>,
) {
    let Some(conn) = &state.connection else {
        return;
    };

    let sync_payload = PlayerInputPayload {
        inputs: input_buffer.history.map(|input| PlayerInput { input }),
    };

    let publish_packet = Publish {
        topic_id: CustomId::from(session.custom_id),
        payload: bitcode::encode(&sync_payload),
    };

    let stream = GameStream::new(
        LogicalStream::Input as u16,
        GameStreamReliability::Unreliable,
    );

    let _ = state.peer.send(conn, &stream, publish_packet.to_bytes());
}
