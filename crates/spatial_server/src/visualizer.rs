// visualizer.rs
use crate::quad_tree::{QuadTree};
use serde::Serialize;

use mmo_math_tools::rect::Rect;


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
    pub margin: f32,
}

#[derive(Serialize)]
pub struct VizPlayer {
    pub id: u32,
    pub x: f32,
    pub y: f32,
}

// --- Logique d'extraction ---

pub fn extract_viz_state(tree: &QuadTree, margin: f32) -> String {
    let mut state = VizState {
        bounds: viz_rect(&tree.bounds),
        shards: Vec::new(),
        players: Vec::new(),
    };

    // Fonction récursive pour parcourir l'arbre
    fn traverse(node: &QuadTree, state: &mut VizState, margin: f32) {
        if let Some(children) = &node.children {
            for child in children.iter() {
                traverse(child, state, margin);
            }
        } else {
            // C'est une feuille (un vrai Shard)
            state.shards.push(VizShard {
                rect: viz_rect(&node.bounds),
                id: node.shard_id.map(|id| id.into()).unwrap_or(0),
                depth: node.depth,
                margin,
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

    traverse(tree, &mut state, margin);
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