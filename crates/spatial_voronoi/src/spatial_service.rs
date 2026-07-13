use std::time::Instant;
use crate::network::PeerType;
use crate::voronoi::{Voronoi, VoronoiEvent};
use crate::shard_id::{ShardId};
use ahash::{AHashMap, AHashSet};
use bytes::Bytes;
use mathtools::Vec2;

use internal_communication_protocol::internal_models::*;
use custom_id::client_id::ClientId;
use crate::shared::Point2D;

pub struct SpatialService {
    pub voronoi: Voronoi,

    /// client_id → (shard_id, last_shard_change_time) (temps pour l'Hystérésis)
    pub client_to_shards: AHashMap<ClientId, (ShardId, Instant)>,

    /// shard_id → client_id: is replicate in ghost on this serv ? (if exist alors le shard est pret à recevoir l'autorité)
    pub ghost_client: AHashMap<ShardId, Vec<ClientId>>,

    /// client_id → (old_shard_id, new_shard_id) : client pas encore repliqué en ghost mais qui a deja cross.
    pub client_waiting_for_crossing: AHashMap<ClientId, (ShardId,ShardId)>,
    
    pub hysteresis_time: f32,
    pub map_width: f32,
    pub map_height: f32,
    pub is_root_initialized: bool,
}

impl SpatialService {
    pub(crate) fn extract_viz_state(&self) -> String {
        self.voronoi.extract_viz_state()
    }
}

impl SpatialService {
    pub fn new(
        margin: f32,
        occupation_to_subdivide: u8,
        occupation_to_merge: u8,
        time_for_merge_after_subdivide: f32,
        hysteresis_time: f32,
        map_width: f32,
        map_height: f32,
    ) -> Self {

        let max_dist = map_width.max(map_height) * 0.25;

        let voronoi_config = crate::voronoi::VoronoiConfig {
            split_occupancy_threshold: occupation_to_subdivide,
            merge_occupancy_threshold: occupation_to_merge,
            min_age_ticks: (time_for_merge_after_subdivide * 5.0) as u64,
            max_merge_dist_sq: max_dist * max_dist,
            ghost_margin: margin,
            map_width,
            map_height,
        };

        Self {
            voronoi: Voronoi::new(5.0, voronoi_config),
            client_to_shards: AHashMap::new(),
            ghost_client: AHashMap::new(),
            client_waiting_for_crossing: AHashMap::new(),
            hysteresis_time,
            map_width,
            map_height,
            is_root_initialized: false,
        }
    }

    pub fn process_player_join(
        &mut self,
        update_data: PlayerJoinUpdate,
    ) -> Option<Vec<(PeerType, Bytes)>> {
        let mut outgoing_packets: Vec<(PeerType, Bytes)> = Vec::new();

        let client_id = ClientId::try_from(update_data.client_id).ok()?;
        println!("client_id: {:?}", client_id);

        if !self.is_root_initialized {
            let refuse_client = RefuseClient {
                client_id: client_id.into()
            };
            outgoing_packets.push((PeerType::Broker, refuse_client.to_bytes()));
            return Some(outgoing_packets)
        }

        let pos: Point2D = update_data.pos.into();

        let Some(shard_id) = self.voronoi.shard_id_for(pos) else {
            println!(
                "error : no shard found for player join position (client_id: {:?}, pos: {:?})",
                client_id, pos
            );
            return None;
        };

        self.voronoi.insert_player(client_id, pos, shard_id);

        self.client_to_shards.insert(client_id, (shard_id, Instant::now()));

        let player_spawn = SpawnPlayerShard {
            shard_id: shard_id.into(),
            client_id: client_id.into(),
            pos: pos.into(),
        };

        let visible_shards_fx = self.voronoi.get_visible_shards_for(pos);
        let visible_shards: Vec<ShardId> = visible_shards_fx.into_iter().collect();

        self.voronoi.set_player_ghost_shards(client_id, visible_shards.clone());

        for near_shard_id in visible_shards {
            if near_shard_id != shard_id {
                outgoing_packets.push(fast_handoff_req(client_id, near_shard_id));
            }
        }

        outgoing_packets.push((PeerType::Broker, player_spawn.to_bytes()));

        Some(outgoing_packets)
    }

    pub fn process_player_left(
        &mut self,
        update_data: ClientLeft,
    ) -> Option<Vec<(PeerType, Bytes)>> {
        let mut outgoing_packets: Vec<(PeerType, Bytes)> = Vec::new();

        let client_id = ClientId::try_from(update_data.client_id).ok()?;

        let Some((shard_id, _)) = self.client_to_shards.remove(&client_id) else {
            println!(
                "error : client_id {:?} not found in client_to_shards (process_player_left)",
                client_id
            );
            return None;
        };

        let Some(player) = self.voronoi.remove_player(client_id) else {
            println!(
                "error : player position not found in quad_tree for client_id {:?} and shard_id {:?} (process_player_left)",
                client_id, shard_id
            );
            // normalement impossible
            return None;
        };

        let despawn_pkt = DespawnPlayerShard {
            shard_id: shard_id.into(),
            client_id: client_id.into(),
        };
        outgoing_packets.push((PeerType::Broker, despawn_pkt.to_bytes()));

        let shard_id_concerned = player.ghost_shards;

        for temp_shard_id in shard_id_concerned {
            let despawn_pkt = DespawnPlayerShard {
                shard_id: temp_shard_id.into(),
                client_id: client_id.into(),
            };
            outgoing_packets.push((PeerType::Broker, despawn_pkt.to_bytes()));
        }

        self.client_waiting_for_crossing.remove(&client_id);

        Some(outgoing_packets)
    }

    pub fn process_position_update(
        &mut self,
        update_data: PositionUpdate,
    ) -> Option<Vec<(PeerType, Bytes)>> {
        let mut outgoing_packets: Vec<(PeerType, Bytes)> = Vec::new();

        let client_id = ClientId::try_from(update_data.client_id).ok()?;
        let new_pos = update_data.pos;
        
        let Some(_old_pos) = self.voronoi.update_position_get_old(client_id, Point2D::from(new_pos)) else {
            println!("error : player {:?} not found in voronoi", client_id);
            return None;
        };

        let current_shard = self.voronoi.get_player_shard(client_id)?;

        if !self.client_waiting_for_crossing.contains_key(&client_id) { // 🛡️ AJOUT ICI
            if let Some(optimal_shard) = self.voronoi.check_single_handoff(client_id) {
                let last_handoff = self.client_to_shards
                    .get(&client_id)
                    .map(|(_, time)| *time)
                    .unwrap_or_else(|| std::time::Instant::now() - std::time::Duration::from_secs(100));

                if last_handoff.elapsed() > std::time::Duration::from_secs_f32(self.hysteresis_time) {
                    println!(
                        "CROSSING ALERT : Joueur {:?} a demandé à changer de shard ({:?} -> {:?})",
                        client_id, current_shard, optimal_shard
                    );

                    outgoing_packets.append(
                        self.apply_client_cross(client_id, current_shard, optimal_shard, new_pos).as_mut()
                    );
                }
            }
        }
        
        let past_visible_shards_vec = self.voronoi.get_player_ghost_shards(client_id).unwrap_or_default();
        let past_visible_shards: AHashSet<ShardId> = past_visible_shards_vec.into_iter().collect();

        let current_visible_shards_fx = self.voronoi.get_visible_shards_for(Point2D::from(new_pos));
        let current_visible_shards: AHashSet<ShardId> = current_visible_shards_fx.into_iter().collect();

        self.voronoi.set_player_ghost_shards(client_id, current_visible_shards.iter().copied().collect());

        for &left_shard in past_visible_shards.difference(&current_visible_shards) {
            let unsub = HandoffDrop {
                shard_id: left_shard.into(),
                entity_id: client_id.into(),
            };
            outgoing_packets.push((PeerType::Broker, unsub.to_bytes()));

            if let Some(ghosts) = self.ghost_client.get_mut(&left_shard) {
                ghosts.retain(|&c| c != client_id);
            }
        }

        for &entered_shard in current_visible_shards.difference(&past_visible_shards) {
            outgoing_packets.push(fast_handoff_req(client_id, entered_shard));
        }

        Some(outgoing_packets)
    }

    pub fn process_handoff_accept(
        &mut self,
        update_data: HandoffAccept,
    ) -> Option<Vec<(PeerType, Bytes)>> {
        let mut outgoing_packets: Vec<(PeerType, Bytes)> = Vec::new();

        let client_id = ClientId::try_from(update_data.entity_id).ok()?;
        let shard_id = ShardId::try_from(update_data.shard_id).ok()?;

        let Some(_) = self.client_to_shards.get(&client_id) else {
            return None;
        };
        
        if let Some(&(old_shard_id, waiting_shard)) = self.client_waiting_for_crossing.get(&client_id) {
            if waiting_shard == shard_id {
                self.client_waiting_for_crossing.remove(&client_id);
                
                let pos = self.voronoi.get_player_pos(client_id)?;

                let handoff_complete = HandoffComplete {
                    new_shard_id: shard_id.into(),
                    entity_id: client_id.into(),
                    old_shard_id: old_shard_id.into(),
                    pos: pos.into(),
                };

                outgoing_packets.push((PeerType::Broker, handoff_complete.to_bytes()));

                self.voronoi.update_player_shard(client_id, shard_id);
                self.client_to_shards.insert(client_id, (shard_id, Instant::now()));

                return Some(outgoing_packets);
            }
        }

        self.ghost_client.entry(shard_id).or_default().push(client_id);
        None
    }

    pub fn process_server_heartbeat(
        &mut self,
        update_data: ServerHeartBeat,
    ) -> Option<Vec<(PeerType, Bytes)>> {
        let shard_id = ShardId::try_from(update_data.shard_id).ok()?;

        self.voronoi.set_shard_occupancy(shard_id, update_data.occupancy);
        println!("Occupancy for shard {:?} : {:?}", shard_id, update_data.occupancy);
        None
    }

    pub fn process_server_spawned(
        &mut self,
        update_data: ServerSpawned,
    ) -> Option<Vec<(PeerType, Bytes)>> {
        let mut outgoing_packets: Vec<(PeerType, Bytes)> = Vec::new();
        let shard_id: ShardId = ShardId::try_from(update_data.shard_id).ok()?;

        println!("Shard id: {:?} spawned", shard_id);

        // Cas spécial du ROOT
        if shard_id == ShardId::ROOT && !self.is_root_initialized {
            self.is_root_initialized = true;
            self.voronoi.init_base_shards(self.map_width / 2.0, self.map_height / 2.0);
            println!("Shard ROOT is initialized.");
            return None;
        }
        
        if let Some(topo_update) = self.voronoi.confirm_server_spawned(shard_id) {
            self.process_topology_commit(&topo_update, &mut outgoing_packets);
        }

        if outgoing_packets.is_empty() { None } else { Some(outgoing_packets) }
    }

    pub fn apply_client_cross(
        &mut self,
        client_id: ClientId,
        old_shard_id: ShardId,
        new_shard_id: ShardId,
        new_pos: Vec2<f32>,
    ) -> Vec<(PeerType, Bytes)> {
        let mut outgoing_packets: Vec<(PeerType, Bytes)> = Vec::new();
        self.client_waiting_for_crossing.remove(&client_id);

        let mut already_ghost = false;
        if let Some(ghost_in_shards) = self.ghost_client.get_mut(&new_shard_id) {
            if ghost_in_shards.contains(&client_id) {
                already_ghost = true;
                ghost_in_shards.retain(|&s| s != client_id);
            }
        }

        if already_ghost {
            self.ghost_client.entry(old_shard_id).or_default().push(client_id);

            let handoff_complete = HandoffComplete {
                new_shard_id: new_shard_id.into(),
                old_shard_id: old_shard_id.into(),
                entity_id: client_id.into(),
                pos: new_pos,
            };
            outgoing_packets.push((PeerType::Broker, handoff_complete.to_bytes()));
            self.voronoi.update_player_shard(client_id, new_shard_id);
            self.client_to_shards.insert(client_id, (new_shard_id, Instant::now()));
        } else {
            self.client_waiting_for_crossing.insert(client_id, (old_shard_id, new_shard_id));
            outgoing_packets.push(fast_handoff_req(client_id, new_shard_id));

            self.client_to_shards.insert(client_id, (old_shard_id, Instant::now()));
        }

        let mut ghosts = self.voronoi.get_player_ghost_shards(client_id).unwrap_or_default();
        if !ghosts.contains(&new_shard_id) {
            ghosts.push(new_shard_id);
            self.voronoi.set_player_ghost_shards(client_id, ghosts);
        }

        outgoing_packets
    }

    pub fn tick(&mut self, dt: f32) -> Option<Vec<(PeerType, Bytes)>> {
        let mut outgoing_packets: Vec<(PeerType, Bytes)> = Vec::new();

        if let Some(event) = self.voronoi.tick(dt) {
            match event {
                VoronoiEvent::SpawnRequests(shards_to_spawn) => {
                    for new_shard in shards_to_spawn {
                        let packet = SpawnServer { shard_id: new_shard.into() };
                        outgoing_packets.push((PeerType::Orchestrator, packet.to_bytes()));
                    }
                }
                VoronoiEvent::TopologyCommitted(update) => {
                    self.process_topology_commit(&update, &mut outgoing_packets);
                }
            }
        }

        if outgoing_packets.is_empty() { None } else { Some(outgoing_packets) }
    }

    fn process_topology_commit(&mut self, topo_update: &crate::voronoi::TopologyUpdate, outgoing_packets: &mut Vec<(PeerType, Bytes)>) {

        for dead_shard in &topo_update.despawned_shards {
            let packet = ShutdownServerOnEmpty { shard_id: (*dead_shard).into() };
            outgoing_packets.push((PeerType::Broker, packet.to_bytes()));
        }
        
        for &(client_id, old_shard, new_shard) in &topo_update.forced_handoffs {
            
            let mut ghosts = self.voronoi.get_player_ghost_shards(client_id).unwrap_or_default();
            ghosts.retain(|&s| s != old_shard);
            self.voronoi.set_player_ghost_shards(client_id, ghosts);

            if let Some(pos) = self.voronoi.get_player_pos(client_id) {
                let mut cross_packets = self.apply_client_cross(
                    client_id,
                    old_shard,
                    new_shard,
                    pos.into()
                );
                outgoing_packets.append(&mut cross_packets);
            }
        }
    }
}

pub fn fast_handoff_req(client_id: ClientId, new_shard_id: ShardId) -> (PeerType, Bytes) {
    let handoff_complete = HandoffRequest {
        shard_id: new_shard_id.into(),
        entity_id: client_id.into(),
    };
    (PeerType::Broker, handoff_complete.to_bytes())
}
