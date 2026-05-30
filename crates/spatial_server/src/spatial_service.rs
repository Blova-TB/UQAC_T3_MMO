use ahash::{AHashMap, AHashSet};
use bytes::Bytes;
use mathtools::Vec2;
use shared::models::{PositionUpdate, ServerBinaryPacket, ServerHandShake, ShutdownServer, SpawnServer, Subscribe, Unsubscribe};
use crate::network::PeerType;
use crate::quad_tree::{QuadTree, Rect};
use crate::shard_id::{Quadrant, ShardId};

pub struct SpatialService {
    pub quad_tree: QuadTree,
    pub client_to_shards: AHashMap<u32, ShardId>,
    pub ghost_client: AHashMap<u32,Vec<ShardId>>, // client_id -> shard_id : is replicate in ghost on this serv ? (if exist alors le shard est pret à recevoir l'autorité)
    pub client_waiting_for_crossing: AHashMap<u32,ShardId>, // client_id -> (shard_id) : client pas encore repliqué en ghost mais qui a deja cross.
    pub margin: f32,
    pub occupation_to_subdivide: f32,
    pub occupation_to_merge: f32,
    pub time_for_merge_after_subdivide: f32,
}

impl SpatialService {
    pub fn new(bounds: Rect, max_depth: u8, margin: f32, occupation_to_subdivide : f32, occupation_to_merge : f32, time_for_merge_after_subdivide : f32
    ) -> Self {

        if max_depth > ShardId::MAX_DEPTH {
            panic!("max_depth cannot be greater than {} (SpatialService init)", ShardId::MAX_DEPTH);
        }

        Self {
            quad_tree: QuadTree::new(bounds, 0, max_depth, ShardId::ROOT),
            client_to_shards: AHashMap::new(),
            ghost_client: AHashMap::new(),
            client_waiting_for_crossing: AHashMap::new(),
            margin,
            occupation_to_subdivide,
            occupation_to_merge,
            time_for_merge_after_subdivide,
        }
    }

    // pub fn process_update(&mut self, update_data: PositionUpdate)->Option<Vec<(PeerType,Bytes)>>{
    //
    //     let mut outgoing_packets : Vec<(PeerType,Bytes)> = Vec::new();
    //
    //     let new_shard_id: ShardId = self.quad_tree.insert_player(update_data.client_id,update_data.pos)?;
    //     let old_shard_id_opt = self.client_to_shards.insert(update_data.client_id, new_shard_id);
    //
    //     if !old_shard_id.is_none() {
    //         self.quad_tree.remove_player(update_data.client_id, old_shard_id?)?;
    //         let unsub = Unsubscribe {
    //             client_id: update_data.client_id,
    //             shard_id: old_shard_id?.into(),
    //         };
    //         outgoing_packets.push((PeerType::Broker ,unsub.to_bytes()));
    //     }
    //
    //     if old_shard_id.is_none() || old_shard_id? != new_shard_id {
    //         let sub = Subscribe {
    //             client_id: update_data.client_id,
    //             shard_id: new_shard_id.into(),
    //         };
    //         outgoing_packets.push((PeerType::Broker ,sub.to_bytes()));
    //     }
    //     Some(outgoing_packets)
    // }

    pub fn process_update(&mut self, update_data: PositionUpdate) -> Option<Vec<(PeerType, Bytes)>> {
        let mut outgoing_packets: Vec<(PeerType, Bytes)> = Vec::new();

        let client_id = update_data.client_id;

        let new_pos = update_data.pos;
        let new_shard_id = self.quad_tree.insert_player(client_id, new_pos)?;

        let old_shard_id_opt = self.client_to_shards.insert(client_id, new_shard_id);
        let old_shard_id : ShardId;

        let old_pos_opt : Option<Vec2<f32>>;
        let old_pos : Vec2<f32>;

        let Some(old_shard_id) = old_shard_id_opt else {
            println!("error : NOUVEAU JOUEUR dans le process_update ?????");
            return None;
        };

        if let Some(old_node) = self.quad_tree.get_shard_by_id(old_shard_id){
            old_pos_opt = old_node.players.get(&client_id).copied();
        } else {
            println!(
                "error : ANCIEN SHARD INEXISTANT dans le process_update ????? (client_id: {}, old_shard_id: {})",
                client_id, old_shard_id.0
            );
            // todo:
            // probablement un cas où le joueur etait dans une shard qui a été subdivisé ou fusionné en meme temps qu'il a change de shard.
            return None;
        }

        let Some(old_pos) = old_pos_opt else {
            println!(
                "error : ANCIEN JOUEUR INEXISTANT dans le process_update ????? (client_id: {}, old_shard_id: {})",
                client_id, old_shard_id.0
            );
            return None;
        };

        if let Some(old_shard_id) = old_shard_id_opt {
            if old_shard_id != new_shard_id {
                self.quad_tree.remove_player(client_id, old_shard_id)?;
                println!(
                    "🚨 CROSSING ALERT : Joueur {} a changé de shard ({} -> {})",
                    client_id, old_shard_id.0, new_shard_id.0
                );

                self.client_waiting_for_crossing.remove(&client_id);

                if let Some(ghost_in_shards) = self.ghost_client.get_mut(&client_id)
                    && ghost_in_shards.contains(&new_shard_id){

                    // Le client est déjà en ghost dans le nouveau shard !!!

                    ghost_in_shards.retain(|&s| s != new_shard_id);
                    ghost_in_shards.push(old_shard_id);

                    //TODO APPELER LES PACKETS DE PACKETS !!!
                }else{

                    // Le client n'est pas en ghost dans le nouveau shard :-(

                    self.client_waiting_for_crossing.insert(client_id, new_shard_id);

                    //TODO APPELER LES PACKETS DE PACKETS !!!
                }
            }
        }


        let current_visible_shards: AHashSet<ShardId> = self.quad_tree
            .shards_near(new_pos, self.margin)
            .into_iter()
            .collect();

        let past_visible_shards: AHashSet<ShardId> = self.quad_tree
            .shards_near(old_pos, self.margin)
            .into_iter()
            .collect();


        // Unsubscribe
        for left_shard in past_visible_shards.difference(&current_visible_shards) {
            let unsub = Unsubscribe {
                client_id,
                shard_id: left_shard.0,
            };
            outgoing_packets.push((PeerType::Broker, unsub.to_bytes()));
        }

        // Subscribe
        for entered_shard in current_visible_shards.difference(&past_visible_shards) {
            let sub = Subscribe {
                client_id,
                shard_id: entered_shard.0,
            };
            outgoing_packets.push((PeerType::Broker, sub.to_bytes()));
        }

        Some(outgoing_packets)
    }

    pub fn process_server_handshake(&mut self, update_data: ServerHandShake) -> Option<Vec<(PeerType,Bytes)>>{

        let shard_id = ShardId::from(update_data.shard_id);

        let mut current_node = &mut self.quad_tree;

        for quadrant in shard_id.id_to_path() {
            current_node = current_node.get_shard(quadrant)?;
        }

        current_node.server_occupation = Option::from(update_data.occupancy);

        if update_data.occupancy > self.occupation_to_subdivide {
            if current_node.depth >= current_node.max_depth {
                print!("Shard {:?} is already at max depth, cannot subdivide further.", shard_id);
                return None;
            }
            return self.subdivide_node(shard_id);
        }

        None
    }

    pub fn subdivide_node(&mut self, shard_id: ShardId) ->Option<Vec<(PeerType, Bytes)>>{

        let mut outgoing_packets : Vec<(PeerType,Bytes)> = Vec::new();

        let mut current_node = &mut self.quad_tree;

        for quadrant in shard_id.id_to_path() {
            current_node = current_node.get_shard(quadrant)?;
        }

        if current_node.depth >= current_node.max_depth {
            print!("Shard {:?} is already at max depth, cannot subdivide further.", shard_id);
            return None;
        }

        let new_sub:Vec<(u32, ShardId)> = current_node.subdivide_quad_tree();
        let mut old_sub:Vec<(u32, ShardId)> = Vec::new();

        for &(player_id, shard_id) in &new_sub {
            if let Some(old_shard) = self.client_to_shards.insert(player_id, shard_id) {
                old_sub.push((player_id, old_shard));
            }
        };

        for(player_id, shard_id) in old_sub.iter() {
            let unsub = Unsubscribe {
                client_id: *player_id,
                shard_id: (*shard_id).into(),
            };
            outgoing_packets.push((PeerType::Broker ,unsub.to_bytes()));
        }

        for(player_id, shard_id) in new_sub.iter() {
            let sub = Subscribe {
                client_id: *player_id,
                shard_id: (*shard_id).into(),
            };
            outgoing_packets.push((PeerType::Broker ,sub.to_bytes()));
        }

        for quadr in Quadrant::get_all() {
            let Rect{ min, max } = quadr.get_bound_from_parent(&current_node.bounds);
            let spawn_server = SpawnServer {
                shard_id: shard_id.new_id_for_child(quadr).into(),
                pos_max: max,
                pos_min: min,
            };
            outgoing_packets.push((PeerType::Orchestrator , spawn_server.to_bytes()));
        }

        let shutdown_server = ShutdownServer {
            shard_id: shard_id.into(),
        };
        outgoing_packets.push((PeerType::Orchestrator , shutdown_server.to_bytes()));

        Some(outgoing_packets)
    }
}