// visualizer.rs
use crate::quad_tree::{QuadTree, Rect};
use serde::Serialize;
use std::net::TcpListener;
use std::sync::mpsc::Receiver;
use std::thread;
use std::time::Duration;
use tungstenite::Message;

// --- Modèles pour le Frontend (JSON) ---

#[derive(Serialize)]
pub struct VizState {
    pub bounds: VizRect,
    pub shards: Vec<VizShard>,
    pub players: Vec<VizPlayer>,
}

#[derive(Serialize)]
pub struct VizRect {
    pub min_x: f32,
    pub min_y: f32,
    pub max_x: f32,
    pub max_y: f32,
}

#[derive(Serialize)]
pub struct VizShard {
    pub rect: VizRect,
    pub id: u32,
    pub depth: u8,
}

#[derive(Serialize)]
pub struct VizPlayer {
    pub id: u32,
    pub x: f32,
    pub y: f32,
}

// --- Logique d'extraction ---

pub fn extract_viz_state(tree: &QuadTree) -> String {
    let mut state = VizState {
        bounds: viz_rect(&tree.bounds),
        shards: Vec::new(),
        players: Vec::new(),
    };

    // Fonction récursive pour parcourir l'arbre
    fn traverse(node: &QuadTree, state: &mut VizState) {
        if let Some(children) = &node.children {
            for child in children.iter() {
                traverse(child, state);
            }
        } else {
            // C'est une feuille (un vrai Shard)
            state.shards.push(VizShard {
                rect: viz_rect(&node.bounds),
                id: node.shard_id.map(|id| id.into()).unwrap_or(0),
                depth: node.depth,
            });

            // Ajouter les joueurs de cette feuille
            for (id, pos) in &node.players {
                state.players.push(VizPlayer {
                    id: (*id).into(),
                    x: pos.x,
                    y: pos.y,
                });
            }
        }
    }

    traverse(tree, &mut state);
    serde_json::to_string(&state).unwrap_or_else(|_| "{}".to_string())
}

fn viz_rect(rect: &Rect) -> VizRect {
    VizRect {
        min_x: rect.min.x,
        min_y: rect.min.y,
        max_x: rect.max.x,
        max_y: rect.max.y,
    }
}

// --- Serveur WebSocket Isolé ---

pub fn start_visualizer_server(addr: &str, receiver: Receiver<String>) {
    let listener = TcpListener::bind(addr).expect("❌ Impossible de bind le port WebSocket");
    listener.set_nonblocking(true).unwrap();

    println!("👁️ [Visualizer] Serveur WebSocket démarré sur ws://{}", addr);

    thread::spawn(move || {
        let mut clients = Vec::new();
        // 🚀 CORRECTION 1 : L'état DOIT persister en dehors de la boucle !
        let mut latest_state: Option<String> = None;

        loop {
            // --- 1. GESTION DES NOUVEAUX CLIENTS ---
            if let Ok((stream, _)) = listener.accept() {
                stream.set_nonblocking(false).unwrap(); // Requis pour le handshake Tungstenite
                if let Ok(mut ws_socket) = tungstenite::accept(stream) {
                    println!("👁️ [Visualizer] Nouveau client connecté !");

                    // 🚀 CORRECTION 2 : On force le socket en NON-BLOQUANT pour l'I/O
                    ws_socket.get_mut().set_nonblocking(true).unwrap();

                    // Envoi immédiat de l'état (plus d'écran vide au démarrage)
                    if let Some(state) = &latest_state {
                        let _ = ws_socket.write(Message::Text(state.clone()));
                    }

                    clients.push(ws_socket);
                }
            }

            let mut has_new_state = false;
            while let Ok(state) = receiver.try_recv() {
                latest_state = Some(state);
                has_new_state = true;
            }

            clients.retain_mut(|client| {
                while let Ok(_) = client.read() {}

                if has_new_state {
                    if let Some(state) = &latest_state {
                        match client.write(Message::Text(state.clone())) {
                            Ok(_) => true,
                            Err(tungstenite::error::Error::Io(e)) if e.kind() == std::io::ErrorKind::WouldBlock => {
                                true
                            }
                            Err(_) => false,
                        }
                    } else {
                        true
                    }
                } else {
                    true
                }
            });

            thread::sleep(Duration::from_millis(50));
        }
    });
}