use ahash::AHashMap;
use mathtools::Vec2;

use internal_communication_protocol::internal_models::{Subscribe, Unsubscribe};
use custom_id::client_id::ClientId;
use aoi_model::aoi_model::*;
use custom_id::chunk_id::ChunkId;

pub struct AoiService {
    ///memoire pour connaitre la position et le mode aoi actuelle de chaque player
    pub players_states :AHashMap<ClientId, PlayerAoiState>,


    ///buffer pour le stocker le resultat de chaque frame.
    pub frame_result: FrameResult,
}

impl AoiService {
    pub fn new() -> Self {
        Self {
            players_states: AHashMap::new(),
            frame_result: FrameResult::new(),
        }
    }

    pub fn process_player_move(
        &mut self,
        client_id: ClientId,
        current_chunk: Vec2<i32>,
    ) -> Result<(), String> {

        self.process_player_move_and_aoi_change(
            client_id,
            current_chunk,
            self.players_states.get(&client_id).map_or(AoiMode::default(), |state| state.aoi_mode),
        )
    }

    pub fn process_player_move_and_aoi_change(
        &mut self,
        client_id: ClientId,
        current_chunk: Vec2<i32>,
        mode: AoiMode,
    ) -> Result<(), String> {
        if let Some(state) = self.players_states.get_mut(&client_id) {
            let old_bounds = state.bounds();

            let mode_changed = state.aoi_mode != mode;
            if mode_changed {
                state.aoi_mode = mode;
            }

            let pos_changed = state.update_pos_if_needed(current_chunk);

            if pos_changed || mode_changed {
                let new_bounds = state.bounds();
                let delta = calculate_aoi_delta(old_bounds, new_bounds);

                self.frame_result.sub.extend(delta.added.into_iter().map(|chunk| Subscribe {
                    custom_id : client_id.into(),
                    topic_id: ChunkId::new(chunk.x,chunk.y).unwrap().into(),
                }));
                self.frame_result.unsub.extend(delta.removed.into_iter().map(|chunk| Unsubscribe {
                    custom_id : client_id.into(),
                    topic_id: ChunkId::new(chunk.x,chunk.y).unwrap().into(),
                }));
                Ok(())

            } else {
                Ok(())
            }
        } else {
            // nouv joueur
            let state = PlayerAoiState::new(current_chunk, mode);
            let bounds = state.bounds();
            self.players_states.insert(client_id, state);

            self.frame_result.sub.extend(bounds.iter_chunks().map(|chunk| Subscribe {
                custom_id : client_id.into(),
                topic_id: ChunkId::new(chunk.x,chunk.y).unwrap().into(),
            }));

            //todo : unsub le nouveau player de tout les chunks et ensuite le sub (pour si jamais il etait deja sub a des chunks)
            // un autre packet a faire probablement
            Ok(())
        }
    }

    pub fn remove_player(&mut self, client_id: ClientId) -> Result<(), String> {

        self.players_states.remove(&client_id);
        Ok(())
        
        // normalement le broker devrait le désabonner de tous automatiquement
        
        // self.players_states.remove(&client_id).map(|state| {
        //     AoiDelta {
        //         added: Vec::new(),
        //         removed: state.bounds().iter_chunks().collect(),
        //     }
        // })
    }
}

pub struct FrameResult {
    pub sub : Vec<Subscribe>,
    pub unsub : Vec<Unsubscribe>,
}

impl FrameResult {
    pub fn new() -> Self {
        Self {
            sub: Vec::new(),
            unsub: Vec::new(),
        }
    }
}

pub struct PlayerAoiState {
    pub pos: Vec2<i32>, // top-left center chunk
    pub aoi_mode: AoiMode,
}

impl PlayerAoiState {
    pub fn new(initial_chunk: Vec2<i32>, mode: AoiMode) -> Self {
        Self {
            pos: initial_chunk,
            aoi_mode: mode,
        }
    }

    /// hystérésis -> si le joueur est toujours dans les 4 carrés du centre, on ne deplace pas l'aoi
    /// return true si la position a changé, false sinon
    pub fn update_pos_if_needed(&mut self, current_chunk: Vec2<i32>) -> bool {
        let mut changed = false;

        if current_chunk.x < self.pos.x {
            self.pos.x = current_chunk.x;
            changed = true;
        } else if current_chunk.x > self.pos.x + 1 {
            self.pos.x = current_chunk.x - 1;
            changed = true;
        }

        if current_chunk.y < self.pos.y {
            self.pos.y = current_chunk.y;
            changed = true;
        } else if current_chunk.y > self.pos.y + 1 {
            self.pos.y = current_chunk.y - 1;
            changed = true;
        }

        changed
    }

    pub fn bounds(&self) -> AoiBounds {
        AoiBounds::from_center_pos(self.pos, self.aoi_mode)
    }
}