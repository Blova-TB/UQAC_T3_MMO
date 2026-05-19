use bevy::app::ScheduleRunnerPlugin;
use bevy::prelude::*;
use bevy::tasks::{block_on, poll_once, AsyncComputeTaskPool, Task};
use bitcode::{Decode, Encode};
use bytes::Bytes;
use std::time::Duration;

use shared::network::{GameConnection, GameNetworkEvent, GamePeer};
use shared::network::protocols::QuicBackend;

#[derive(Encode, Decode, Debug)]
pub enum ClientPacket {
    Join { username: String },
}

#[derive(Encode, Decode, Debug)]
pub enum ServerPacket {
    Welcome { player_id: u64 },
    RejectedFull,
    SyncPositions(Vec<PlayerPositionData>),
}

#[derive(Encode, Decode, Debug)]
pub struct PlayerPositionData {
    pub entity_bits: u64,
    pub position: [f32; 2],
}

#[derive(States, Default, Debug, Clone, Eq, PartialEq, Hash)]
pub enum AppState {
    #[default]
    GatekeeperLogin, // Étape 1 : Authentification HTTP Basic -> Récupération du JWT
    GatekeeperFetch, // Étape 2 : Requête HTTP GET /server avec le JWT Bearer
    Connecting,      // Étape 3 : Handshake QUIC et envoi du Join
    InGame,          // Étape 4 : Réception de la simulation physique
}

#[derive(Resource)]
pub struct GatekeeperToken(pub String);

#[derive(Resource)]
pub struct TargetServer {
    pub ip: String,
    pub port: u16,
}

#[derive(Resource)]
pub struct ClientState {
    pub peer: GamePeer,
    pub connection: Option<GameConnection>,
    pub joined: bool,
}

#[derive(Component)]
struct LoginTask(Task<Result<String, reqwest::Error>>);

#[derive(Component)]
struct FetchServerTask(Task<Result<String, reqwest::Error>>);

fn main() {
    App::new()
        .add_plugins(MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(
            Duration::from_secs_f64(1.0 / 60.0),
        )))
        .init_state::<AppState>()

        // Phase 1 : Login Gatekeeper
        .add_systems(OnEnter(AppState::GatekeeperLogin), spawn_login_request)
        .add_systems(Update, poll_login_request.run_if(in_state(AppState::GatekeeperLogin)))

        // Phase 2 : Récupération de l'adresse du serveur
        .add_systems(OnEnter(AppState::GatekeeperFetch), spawn_gatekeeper_request)
        .add_systems(Update, poll_gatekeeper_request.run_if(in_state(AppState::GatekeeperFetch)))

        // Phase 3 : Connexion QUIC
        .add_systems(OnEnter(AppState::Connecting), init_game_connection)
        .add_systems(Update, handle_connection_handshake.run_if(in_state(AppState::Connecting)))

        // Phase 4 : En Jeu
        .add_systems(Update, handle_ingame_network.run_if(in_state(AppState::InGame)))

        .run();
}

// --- Systèmes : Phase 1 (Gatekeeper Login) ---

fn spawn_login_request(mut commands: Commands) {
    println!("Client : Authentification auprès du Gatekeeper (POST /login)...");
    let thread_pool = AsyncComputeTaskPool::get();

    // Identifiants de test à adapter selon votre base de données PostgreSQL
    let username = "test_user".to_string();
    let password = "password123".to_string();

    let task = thread_pool.spawn(async move {
        let client = reqwest::Client::new();
        let res = client
            .post("http://127.0.0.1:8000/login")
            .basic_auth(username, Some(password)) // Injection dans l'en-tête pour BasicCredentials
            .send()
            .await?;

        res.text().await
    });

    commands.spawn(LoginTask(task));
}

fn poll_login_request(
    mut commands: Commands,
    mut q_task: Query<(Entity, &mut LoginTask)>,
    mut next_state: ResMut<NextState<AppState>>,
) {
    for (entity, mut task) in &mut q_task {
        if let Some(result) = block_on(poll_once(&mut task.0)) {
            commands.entity(entity).despawn();

            match result {
                Ok(token) => {
                    if token.contains("Utilisateur non trouvé") || token.contains("Identifiants invalides") {
                        eprintln!("Client : Échec d'authentification : {}", token);
                        return;
                    }
                    println!("Client : Authentifié avec succès. Token JWT obtenu.");
                    commands.insert_resource(GatekeeperToken(token));
                    next_state.set(AppState::GatekeeperFetch);
                }
                Err(e) => {
                    eprintln!("Client : Erreur réseau lors du login: {:?}", e);
                }
            }
        }
    }
}

// --- Systèmes : Phase 2 (Gatekeeper Fetch Server) ---

fn spawn_gatekeeper_request(mut commands: Commands, token: Res<GatekeeperToken>) {
    println!("Client : Requête d'un serveur de jeu (GET /server avec JWT)...");
    let thread_pool = AsyncComputeTaskPool::get();
    let jwt_token = token.0.clone();

    let task = thread_pool.spawn(async move {
        let client = reqwest::Client::new();
        let res = client
            .get("http://127.0.0.1:8000/server")
            .bearer_auth(jwt_token) // Sécurisation obligatoire suite au Request Guard AuthenticatedUser
            .send()
            .await?;

        res.text().await
    });

    commands.spawn(FetchServerTask(task));
}

fn poll_gatekeeper_request(
    mut commands: Commands,
    mut q_task: Query<(Entity, &mut FetchServerTask)>,
    mut next_state: ResMut<NextState<AppState>>,
) {
    for (entity, mut task) in &mut q_task {
        if let Some(result) = block_on(poll_once(&mut task.0)) {
            commands.entity(entity).despawn();

            match result {
                Ok(address) => {
                    println!("Client : Adresse de serveur dédiée allouée -> {}", address);

                    let parts: Vec<&str> = address.split(':').collect();
                    if parts.len() == 2 {
                        if let Ok(port) = parts[1].parse::<u16>() {
                            commands.insert_resource(TargetServer {
                                ip: parts[0].to_string(),
                                port
                            });
                            next_state.set(AppState::Connecting);
                        } else {
                            eprintln!("Erreur : Port invalide retourné par le Gatekeeper : {}", parts[1]);
                        }
                    } else {
                        eprintln!("Erreur : Format d'adresse réseau invalide : {}", address);
                    }
                }
                Err(e) => {
                    eprintln!("Client : Échec d'interrogation du Gatekeeper /server: {:?}", e);
                }
            }
        }
    }
}

// --- Systèmes : Phase 3 (Connexion QUIC) ---

fn init_game_connection(mut commands: Commands, target: Res<TargetServer>) {
    let backend = QuicBackend::new();
    let peer = GamePeer::new(backend);

    println!("Client : Établissement de la liaison QUIC vers {}:{}...", target.ip, target.port);
    peer.connect(&target.ip, target.port).expect("Échec de l'initialisation de la connexion");

    commands.insert_resource(ClientState {
        peer,
        connection: None,
        joined: false,
    });
}

fn handle_connection_handshake(
    mut state: ResMut<ClientState>,
    mut next_state: ResMut<NextState<AppState>>,
    mut exit: MessageWriter<AppExit>,
) {
    loop {
        match state.peer.poll() {
            Ok(Some(event)) => match event {
                GameNetworkEvent::Connected(conn) => {
                    println!("Client : Connecté au serveur QUIC. En attente de l'ouverture du flux...");
                    state.connection = Some(conn);
                }
                GameNetworkEvent::StreamCreated(conn, stream) => {
                    if !state.joined && stream.is_reliable() {
                        println!("Client : Flux Fiable reçu. Envoi de la requête de connexion (Join)...");
                        state.joined = true;

                        let packet = ClientPacket::Join {
                            username: "TestPlayer_01".to_string(),
                        };

                        let encoded_data = bitcode::encode(&packet);
                        if let Err(e) = state.peer.send(&conn, &stream, Bytes::from(encoded_data)) {
                            eprintln!("Client : Échec de l'envoi du paquet: {:?}", e);
                        }
                    }
                }
                GameNetworkEvent::Message { data, .. } => {
                    if let Ok(packet) = bitcode::decode::<ServerPacket>(&data) {
                        match packet {
                            ServerPacket::Welcome { player_id } => {
                                println!("Client : Succès ! Le serveur m'a assigné l'ID : {}", player_id);
                                next_state.set(AppState::InGame);
                            }
                            ServerPacket::RejectedFull => {
                                println!("Client : Connexion refusée (Serveur plein).");
                                exit.write(AppExit::Success);
                            }
                            _ => {}
                        }
                    }
                }
                GameNetworkEvent::Disconnected(_) => {
                    println!("Client : Déconnecté par le serveur.");
                    exit.write(AppExit::Success);
                }
                GameNetworkEvent::Error { inner, .. } => {
                    eprintln!("Client : Erreur protocole : {:?}", inner);
                }
                _ => {}
            },
            Ok(None) => break,
            Err(e) => {
                eprintln!("Client : Erreur fatale de polling : {:?}", e);
                break;
            }
        }
    }
}

// --- Systèmes : Phase 4 (In Game Execution) ---

fn handle_ingame_network(
    mut session: ResMut<ClientState>,
    mut exit: MessageWriter<AppExit>
) {
    loop {
        match session.peer.poll() {
            Ok(Some(event)) => match event {
                GameNetworkEvent::Message { data, .. } => {
                    if let Ok(packet) = bitcode::decode::<ServerPacket>(&data) {
                        match packet {
                            ServerPacket::SyncPositions(players) => {
                                println!("--- State Sync Snapshot [{} Joueur(s) Connecté(s)] ---", players.len());
                                for player in players {
                                    println!(
                                        " > Entity ID: {:<10} | Position Translatée: [X: {:>6.2}, Y: {:>6.2}]",
                                        player.entity_bits,
                                        player.position[0],
                                        player.position[1]
                                    );
                                }
                            }
                            _ => {}
                        }
                    }
                }
                GameNetworkEvent::Disconnected(_) => {
                    println!("Client : Session fermée par le serveur de jeu dédié.");
                    exit.write(AppExit::Success);
                }
                GameNetworkEvent::Error { inner, .. } => {
                    eprintln!("Client : Perte de paquets ou erreur d'exécution réseau : {:?}", inner);
                }
                _ => {}
            },
            Ok(None) => break,
            Err(_) => break,
        }
    }
}