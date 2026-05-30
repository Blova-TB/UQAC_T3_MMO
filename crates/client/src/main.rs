use bevy::app::ScheduleRunnerPlugin;
use bevy::state::app::StatesPlugin;
use bevy::prelude::*;
use tokio::runtime::Runtime;
use bevy::tasks::{block_on, poll_once, AsyncComputeTaskPool, Task};
use bytes::{Buf, BufMut, Bytes, BytesMut};
use std::time::Duration;
use bevy::tasks::IoTaskPool;

use shared::network::{GameConnection, GameNetworkEvent, GamePeer, GameStream, GameStreamReliability};
use shared::network::protocols::QuicBackend;
use shared::models::{ClientPacket, ServerPacket, ServerSyncMessage, PlayerPositionData};

use rand::random;

#[derive(Resource)]
pub struct TokioRuntime(pub Runtime);

impl Default for TokioRuntime {
    fn default() -> Self {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("Échec de l'initialisation du Runtime Tokio");
        Self(rt)
    }
}

#[derive(States, Default, Debug, Clone, Eq, PartialEq, Hash)]
pub enum AppState {
    #[default]
    GatekeeperLogin, // Étape 1 : Authentification HTTP Basic -> Récupération du JWT
    GatekeeperFetch, // Étape 2 : Requête HTTP GET /server avec le JWT Bearer
    Connecting,      // Étape 3 : Handshake QUIC (Tag 0x00) vers le Broker
    InGame,          // Étape 4 : Réception de la simulation physique (Tag 0x04)
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
}

#[derive(Resource)]
pub struct LocalPlayerId(pub u32);

#[derive(Component)]
struct LoginTask(Task<Result<String, reqwest::Error>>);

#[derive(Component)]
struct FetchServerTask(Task<Result<String, reqwest::Error>>);

fn main() {
    App::new()
        .add_plugins(MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(
            Duration::from_secs_f64(1.0 / 60.0),
        )))
        .add_plugins(StatesPlugin)
        .init_resource::<TokioRuntime>()
        .init_state::<AppState>()

        // Phase 1 : Login Gatekeeper
        .add_systems(OnEnter(AppState::GatekeeperLogin), spawn_login_request)
        .add_systems(Update, poll_login_request.run_if(in_state(AppState::GatekeeperLogin)))

        // Phase 2 : Récupération de l'adresse du serveur (Broker)
        .add_systems(OnEnter(AppState::GatekeeperFetch), spawn_gatekeeper_request)
        .add_systems(Update, poll_gatekeeper_request.run_if(in_state(AppState::GatekeeperFetch)))

        // Phase 3 : Connexion QUIC & Handshake
        .add_systems(OnEnter(AppState::Connecting), init_game_connection)
        .add_systems(Update, handle_connection_handshake.run_if(in_state(AppState::Connecting)))

        // Phase 4 : En Jeu (Réception des Broadcasts)
        .add_systems(Update, handle_ingame_network.run_if(in_state(AppState::InGame)))

        .run();
}

// --- Systèmes : Phase 1 (Gatekeeper Login) ---

fn spawn_login_request(mut commands: Commands, rt: Res<TokioRuntime>) {
    println!("Client : Authentification auprès du Gatekeeper (POST /login)...");

    let username = "test_user".to_string();
    let password = "password123".to_string();

    let join_handle = rt.0.spawn(async move {
        let client = reqwest::Client::new();
        let res = client
            .post("http://gatekeeper:3000/login")
            .basic_auth(username, Some(password))
            .send()
            .await?;

        res.text().await
    });

    let thread_pool = IoTaskPool::get();
    let task = thread_pool.spawn(async move {
        join_handle.await.expect("Le thread Tokio a crashé lors du login")
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

fn spawn_gatekeeper_request(
    mut commands: Commands,
    token: Res<GatekeeperToken>,
    rt: Res<TokioRuntime>
) {
    println!("Client : Requête d'un serveur de jeu (GET /server avec JWT)...");

    let jwt_token = token.0.clone();

    let join_handle = rt.0.spawn(async move {
        let client = reqwest::Client::new();
        let res = client
            .get("http://gatekeeper:3000/server")
            .bearer_auth(jwt_token)
            .send()
            .await?;

        res.text().await
    });

    let thread_pool = IoTaskPool::get();
    let task = thread_pool.spawn(async move {
        join_handle.await.expect("Le thread Tokio a crashé lors du fetch serveur")
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
                    println!("Client : Adresse allouée par le Gatekeeper -> {}", address);

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

// --- Systèmes : Phase 3 (Connexion QUIC & Handshake) ---

fn init_game_connection(mut commands: Commands, target: Res<TargetServer>) {
    let backend = QuicBackend::new();
    let peer = GamePeer::new(backend);

    println!("Client : Établissement de la liaison QUIC vers {}:{}...", target.ip, target.port);
    peer.connect(&target.ip, target.port).expect("Échec de l'initialisation de la connexion");

    commands.insert_resource(ClientState {
        peer,
        connection: None,
    });
}

fn handle_connection_handshake(
    mut commands: Commands,
    mut state: ResMut<ClientState>,
    mut next_state: ResMut<NextState<AppState>>,
    mut exit: MessageWriter<AppExit>,
) {
    loop {
        match state.peer.poll() {
            Ok(Some(event)) => match event {
                // ÉTAPE 1 : La connexion brute est établie
                GameNetworkEvent::Connected(conn) => {
                    println!("Client : Connecté au Broker QUIC. Demande d'un flux fiable pour le Handshake...");
                    state.connection = Some(conn);

                    // On ordonne à la librairie d'ouvrir un canal garanti (TCP-like)
                    if let Err(e) = state.peer.create_stream(conn, GameStreamReliability::Reliable) {
                        eprintln!("Client : Échec lors de la demande de flux fiable : {:?}", e);
                    }
                }

                // ÉTAPE 2 : La librairie nous confirme que le canal réseau est prêt
                GameNetworkEvent::StreamCreated(conn, stream) => {
                    if stream.is_reliable() {
                        println!("Client : Flux fiable ouvert avec succès ! Envoi du Handshake...");

                        // Arbitraire pour tes tests locaux
                        let my_client_id: u32 = random();
                        commands.insert_resource(LocalPlayerId(my_client_id));

                        // Construction du Tag 0x00 (Handshake)
                        let mut handshake_msg = BytesMut::with_capacity(6);
                        handshake_msg.put_u8(0x00); // Tag 0x00 : Handshake
                        handshake_msg.put_u8(0x00); // is_shard = 0 (Joueur)
                        handshake_msg.put_u32_le(my_client_id); // ID (Little-Endian)

                        // L'envoi fonctionnera à 100% car le `stream` existe officiellement
                        if let Err(e) = state.peer.send(&conn, &stream, handshake_msg.freeze().into()) {
                            eprintln!("Client : Échec de l'envoi du Handshake : {:?}", e);
                        } else {
                            println!("Client : Handshake envoyé et garanti ! Passage en mode InGame.");
                            next_state.set(AppState::InGame);
                        }
                    }
                }

                GameNetworkEvent::Disconnected(_) => {
                    println!("Client : Déconnecté par le serveur durant le handshake.");
                    exit.write(AppExit::Success);
                }
                GameNetworkEvent::Error { inner, .. } => {
                    eprintln!("Client : Erreur protocole : {:?}", inner);
                }
                _ => {} // On ignore les autres events (comme de potentiels messages reçus trop tôt)
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
                GameNetworkEvent::Message { mut data, .. } => {
                    if data.is_empty() { continue; }

                    let tag = data.get_u8();

                    match tag {
                        // TAG 0x04 : Broadcast venant du Broker
                        0x04 => {
                            if data.remaining() < 2 { continue; }

                            // 1. Lire la taille du payload bitcode (en Little-Endian)
                            let payload_len = data.get_u16_le() as usize;
                            if data.remaining() < payload_len { continue; }

                            // 2. Extraire les octets correspondants à la snapshot
                            let inner_payload = data.copy_to_bytes(payload_len);

                            // 3. Décoder la snapshot de la Shard
                            if let Ok(sync_msg) = bitcode::decode::<ServerSyncMessage>(&inner_payload) {
                                println!("--- State Sync Snapshot [{} Joueur(s) Visibles] ---", sync_msg.players.len());
                                // Pour afficher les infos, tu peux dé-commenter :
                                /*
                                for player in sync_msg.players {
                                    println!(
                                        " > Entity ID: {:<10} | Position Translatée: [X: {:>6.2}, Y: {:>6.2}]",
                                        player.entity_bits,
                                        player.position[0],
                                        player.position[1]
                                    );
                                }*/
                            }
                        }
                        _ => {
                            // On ignore les tags non gérés par le client pour le moment
                        }
                    }
                }
                GameNetworkEvent::Disconnected(_) => {
                    println!("Client : Session fermée par le Broker.");
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