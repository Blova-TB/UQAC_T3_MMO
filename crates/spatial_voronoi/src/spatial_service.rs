use std::time::Instant;
use crate::client_id::ClientId;
use crate::network::PeerType;
use crate::voronoi::{Voronoi, VoronoiConfig};
use crate::shard_id::{ShardId, ShardIdGenerator};
use ahash::{AHashMap, AHashSet};
use bytes::Bytes;
use mathtools::Vec2;

use internal_communication_protocol::internal_models::*;
use crate::shared::Point2D;

pub struct SpatialService {
    pub voronoi: Voronoi,

    /// client_id → (shard_id, last_shard_change_time) (temps pour l'Hystérésis)
    pub client_to_shards: AHashMap<ClientId, (ShardId, Instant)>,

    /// shard_id → client_id: is replicate in ghost on this serv ? (if exist alors le shard est pret à recevoir l'autorité)
    pub ghost_client: AHashMap<ShardId, Vec<ClientId>>,

    /// client_id → (old_shard_id, new_shard_id) : client pas encore repliqué en ghost mais qui a deja cross.
    pub client_waiting_for_crossing: AHashMap<ClientId, (ShardId,ShardId)>,

    /// shard_id : shard qui a demandé une subdivision et qui attend que tous ses enfants soient spawné pour faire le subdivideNode
    pub shard_waiting_for_subdivide: AHashMap<ShardId, Vec<(ShardId, bool)>>,

    /// shard_id : shard qui a demandé une fusion et qui attend que le shard parent soit spawné pour faire le mergeNode
    pub shard_waiting_for_merge: AHashSet<ShardId>,

    pub margin: f32,
    pub occupation_to_subdivide: u8,
    pub occupation_to_merge: u8,
    pub time_for_merge_after_subdivide: f32,
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
        Self {
            voronoi: Voronoi::default(),
            client_to_shards: AHashMap::new(),
            ghost_client: AHashMap::new(),
            client_waiting_for_crossing: AHashMap::new(),
            shard_waiting_for_subdivide: AHashMap::new(),
            shard_waiting_for_merge: AHashSet::new(),
            margin,
            occupation_to_subdivide,
            occupation_to_merge,
            time_for_merge_after_subdivide,
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

        /*
        TODO handoff
        self.voronoi
            .shards_near(pos, self.margin)
            .into_iter()
            .for_each(|near_shard_id| {
                if near_shard_id != shard_id {
                    outgoing_packets.push(fast_handoff_req(client_id, near_shard_id));
                }
            });
         */

        outgoing_packets.push((PeerType::Broker, player_spawn.to_bytes()));

        Some(outgoing_packets)
    }

    pub fn process_player_left(
        &mut self,
        update_data: ClientLeft,
    ) -> Option<Vec<(PeerType, Bytes)>> {
        /*
        let mut outgoing_packets: Vec<(PeerType, Bytes)> = Vec::new();

        let client_id = ClientId::try_from(update_data.client_id).ok()?;

        let Some((shard_id, _)) = self.client_to_shards.remove(&client_id) else {
            println!(
                "error : client_id {:?} not found in client_to_shards (process_player_left)",
                client_id
            );
            // normalement impossible
            return None;
        };

        let Some(pos) = self.quad_tree.remove_player(client_id, shard_id) else {
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

        // on prend tous les shards dans la marge du player (un peu plus) pour les prévenir de son départ
        let shard_id_concerned = self.quad_tree.shards_near(pos, self.margin * 2f32);

        for temp_shard_id in shard_id_concerned {
            if self.ghost_client.get(&temp_shard_id).map_or(false, |ghosts| ghosts.contains(&client_id)) {

                self.ghost_client.get_mut(&temp_shard_id)?.retain(|&s| s != client_id);

                let despawn_pkt = DespawnPlayerShard {
                    shard_id: temp_shard_id.into(),
                    client_id: client_id.into(),
                };
                outgoing_packets.push((PeerType::Broker, despawn_pkt.to_bytes()));
            }
        }

        self.client_waiting_for_crossing.remove(&client_id);

        Some(outgoing_packets)
        */
        None
    }

    pub fn process_position_update(
        &mut self,
        update_data: PositionUpdate,
    ) -> Option<Vec<(PeerType, Bytes)>> {
        /*
        let mut outgoing_packets: Vec<(PeerType, Bytes)> = Vec::new();

        let client_id = ClientId::try_from(update_data.client_id).ok()?;

        let new_pos = update_data.pos;

        let new_shard_id = self.quad_tree.shard_id_for(new_pos)?;

        let Some(old_shard_id) = self.client_to_shards.get(&client_id) else {
            println!("error : NOUVEAU JOUEUR dans le process_update ?????");
            return None;
        };

        let Some(old_pos) = self.quad_tree.hard_update_player_position(client_id, old_shard_id.0, new_pos) else {
            println!(
                "error : player position not found in quad_tree for client_id {:?} and shard_id {:?} (process_position_update)",
                client_id, old_shard_id
            );
            // normalement impossible
            return None;
        };

        if old_shard_id.0 != new_shard_id
            && old_shard_id.1.elapsed() > std::time::Duration::from_secs_f32(self.hysteresis_time)
        {
            println!(
                "CROSSING ALERT : Joueur {:?} a changé de shard ({:?} -> {:?})",
                client_id, old_shard_id.0, new_shard_id
            );

            outgoing_packets.append(
                self.apply_client_cross(
                    client_id,
                    old_shard_id.0,
                    new_shard_id,
                    new_pos
                ).as_mut()
            );
        }

        let current_visible_shards: AHashSet<ShardId> = self
            .quad_tree
            .shards_near(new_pos, self.margin)
            .into_iter()
            .collect();

        let past_visible_shards: AHashSet<ShardId> = self
            .quad_tree
            .shards_near(old_pos, self.margin)
            .into_iter()
            .collect();

        // Unsubscribe
        for &left_shard in past_visible_shards.difference(&current_visible_shards) {
            let unsub = HandoffDrop {
                shard_id: left_shard.into(),
                entity_id: client_id.into(),
            };
            outgoing_packets.push((PeerType::Broker, unsub.to_bytes()));
        }

        // Subscribe
        for &entered_shard in current_visible_shards.difference(&past_visible_shards) {
            outgoing_packets.push(fast_handoff_req(client_id, entered_shard));
        }

        Some(outgoing_packets)

         */
        None
    }

    pub fn process_handoff_accept(
        &mut self,
        update_data: HandoffAccept,
    ) -> Option<Vec<(PeerType, Bytes)>> {
        /*
        let client_id = ClientId::try_from(update_data.entity_id).ok()?;
        let shard_id = ShardId::try_from(update_data.shard_id).ok()?;

        let Some(_) = self.client_to_shards.get(&client_id) else {
            println!(
                "error : client_id {:?} not found in client_to_shards (process_handoff_accept)",
                client_id
            );
            // vraiment improbable comme situation :
            // probablement un client qui s'est déconnecté pendant le handoff.
            return None;
        };

        if let Some(&(old_shard_id, waiting_shard)) = self.client_waiting_for_crossing.get(&client_id) {
            if waiting_shard == shard_id {
                // si le client attendait pour cross dans cette shard
                self.client_waiting_for_crossing.remove(&client_id);

                let &new_pos= self.quad_tree.get_shard_by_id(shard_id)?.players.get(&client_id)?;

                let handoff_complete = HandoffComplete {
                    new_shard_id: shard_id.into(),
                    entity_id: client_id.into(),
                    old_shard_id: old_shard_id.into(),
                    pos : new_pos,
                };

                return Some(vec![(PeerType::Broker, handoff_complete.to_bytes())]);
            }
        }

        self.ghost_client
            .entry(shard_id)
            .or_default()
            .push(client_id);
        None

         */
        None
    }

    pub fn process_server_heartbeat(
        &mut self,
        update_data: ServerHeartBeat,
    ) -> Option<Vec<(PeerType, Bytes)>> {
        /*
        let mut outgoing_packets: Vec<(PeerType, Bytes)> = Vec::new();
        let shard_id: ShardId = ShardId::try_from(update_data.shard_id).ok()?;

        // on vérifie que le shard_id existe bien dans le quad_tree, sinon c'est chelou

        let Some(current_node) = self.quad_tree.get_shard_by_id_mut(shard_id) else {
            println!(
                "error : shard_id {:?} not found in quad_tree (process_server_health_check)",
                shard_id
            );
            return None;
        };

        let Some(_) = current_node.shard_id else {
            println!(
                "error : shard_id {:?} is not a leaf shard in quad_tree (process_server_health_check)",
                shard_id
            );
            return None;
        };

        current_node.server_occupation = Option::from(update_data.occupancy);
        println!("Server occupation : {:?}", current_node.server_occupation);

        if update_data.occupancy > self.occupation_to_subdivide {
            if current_node.depth >= current_node.max_depth {
                print!(
                    "Shard {:?} is already at max depth, cannot subdivide further.",
                    shard_id
                );
                return None;
            }
            if self.shard_waiting_for_subdivide.contains_key(&shard_id) {
                println!(
                    "Shard {:?} is already waiting for subdivide, cannot request another subdivision.",
                    shard_id
                );
                return None;
            }
            if current_node.creation_date.elapsed().as_secs_f32() < self.time_for_merge_after_subdivide {
                println!(
                    "Shard {:?} was recently subdivided or merge, waiting before requesting another subdivision.",
                    shard_id
                );
                return None;
            }

            outgoing_packets.append(self.subdivide_request(shard_id)?.as_mut());

        } else if (shard_id != ShardId::ROOT) && update_data.occupancy < self.occupation_to_merge {

            let Some(parent_shard_id) = shard_id.get_parent_shard_id() else {
                println!(
                    "shard_id != ShardId::ROOT but YOU ARE THE ROOT SHARD ??? SO COOOOL !!! but ... no merge for u (process_server_health_check)"
                );
                return None;
            };

            if let Some(parent_node) = self.quad_tree.get_shard_by_id_mut(parent_shard_id) {

                if self.shard_waiting_for_merge.contains(&parent_shard_id) {
                    println!(
                        "Parent Shard {:?} is already waiting for merge, cannot request another merge.",
                        parent_shard_id
                    );
                    return None;
                }

                if parent_node.get_occupation_somme(self.time_for_merge_after_subdivide) < self.occupation_to_merge as u32 {
                    // on verifie si un parrent du node n'est pas deja en attente de merge.

                    let mut useless_serv:Vec<ShardId> = Vec::new();

                    for merging_shard_id in self.shard_waiting_for_merge.iter() {
                        if merging_shard_id.is_ancestor_of(parent_shard_id) {
                            println!(
                                "Parent Shard {:?} has an ancestor {:?} already waiting for merge, cannot request another merge.",
                                parent_shard_id, merging_shard_id
                            );
                            return None;
                        }
                        if merging_shard_id.is_descendant_of(parent_shard_id) {
                            println!(
                                "Parent Shard {:?} has a descendant {:?} already waiting for merge",
                                parent_shard_id, merging_shard_id
                            );
                            useless_serv.push(*merging_shard_id);
                        }
                    }

                    for useless in useless_serv.iter() {
                        let useless_serv_packet = ShutdownServerOnEmpty {
                            shard_id: (*useless).into(),
                        };
                        outgoing_packets.push((PeerType::Orchestrator, useless_serv_packet.to_bytes()));
                    }

                    self.shard_waiting_for_merge.retain(|s| !useless_serv.contains(s));

                    outgoing_packets.append(self.merge_request(parent_shard_id)?.as_mut());
                }
            }
        }

        Some(outgoing_packets)

         */
        None
    }

    pub fn process_server_spawned(
        &mut self,
        update_data: ServerSpawned,
    ) -> Option<Vec<(PeerType, Bytes)>> {
        let mut outgoing_packets: Vec<(PeerType, Bytes)> = Vec::new();

        let shard_id: ShardId = ShardId::try_from(update_data.shard_id).ok()?;
        println!("Shard id: {:?} spawned", shard_id);

        if shard_id == ShardId::ROOT && !self.is_root_initialized {
            self.is_root_initialized = true;
            self.voronoi.init_base_shards(self.map_width/2.0, self.map_height/2.0 );
            println!("Shard ROOT is initialized.");
            return None;
        }
        /*
        if let Some(parent_shard_id) = shard_id.get_parent_shard_id()
            && let Some(children) = self.shard_waiting_for_subdivide.get_mut(&parent_shard_id)
        {
            if let Some(child) = children.iter_mut().find(|(id, _)| *id == shard_id) {
                child.1 = true;
            } else {
                println!(
                    "error : shard_id {:?} not found in waiting_for_subdivide for parent_shard_id {:?} (process_server_spawned)",
                    shard_id, parent_shard_id
                );
                return None;
            }

            if children.iter().all(|&(_, spawned)| spawned) {
                self.shard_waiting_for_subdivide.remove(&parent_shard_id);

                outgoing_packets.append(self.subdivide_node(parent_shard_id)?.as_mut());
            }
        } else if let Some(_) = self.shard_waiting_for_merge.get(&shard_id) {
            self.shard_waiting_for_merge.remove(&shard_id);
            outgoing_packets.append(self.merge_node(shard_id)?.as_mut());
        } else {
            println!(
                "error : shard_id {:?} not found in any waiting list (process_server_spawned)",
                shard_id
            );
        }

        Some(outgoing_packets)

         */
        None
    }

    pub fn subdivide_request(&mut self, shard_id: ShardId) -> Option<Vec<(PeerType, Bytes)>> {
        /*
        let mut outgoing_packets: Vec<(PeerType, Bytes)> = Vec::new();

        let current_bounds = &self.quad_tree.get_shard_by_id_mut(shard_id)?.bounds;

        for quads in Quadrant::get_all() {
            let Rect { min, max } = quads.get_bound_from_parent(current_bounds);
            let spawn_server = SpawnServer {
                shard_id: shard_id.new_id_for_child(quads).into(),
            };

            outgoing_packets.push((PeerType::Orchestrator, spawn_server.to_bytes()));
        }

        self.shard_waiting_for_subdivide.insert(
            shard_id,
            Vec::from([
                (shard_id.new_id_for_child(Quadrant::TopLeft), false),
                (shard_id.new_id_for_child(Quadrant::TopRight), false),
                (shard_id.new_id_for_child(Quadrant::BottomLeft), false),
                (shard_id.new_id_for_child(Quadrant::BottomRight), false),
            ]),
        );

        Some(outgoing_packets)

         */
        None
    }

    pub fn subdivide_node(&mut self, shard_id: ShardId) -> Option<Vec<(PeerType, Bytes)>> {
        /*
        let mut outgoing_packets: Vec<(PeerType, Bytes)> = Vec::new();

        let current_node = self.quad_tree.get_shard_by_id_mut(shard_id)?;

        if current_node.depth >= current_node.max_depth {
            println!(
                "Shard {:?} is already at max depth, cannot subdivide further.",
                shard_id
            );
            return None;
        }

        let push_handoffs = |entity_id: ClientId,
                             near_shards: Vec<ShardId>,
                             packets: &mut Vec<(PeerType, Bytes)>| {
            for near_shard_id in near_shards {
                println!("Fast Handoff entity:{:?} shard_id:{:?}", entity_id, near_shard_id);
                packets.push(fast_handoff_req(entity_id, near_shard_id));
            }
        };

        let (players,player_loss) = current_node.subdivide_quad_tree();

        // on prend tous les joueurs et on les met dans self.client_waiting_for_crossing
        // dé qu'ils sont accepté dans le nouveau shard (handoffAccept), ils changenet de shard

        for (player_id, player_new_shard_id, player_pos) in players {
            self.client_waiting_for_crossing
                .insert(player_id, (shard_id,player_new_shard_id));

            self.client_to_shards
                .insert(player_id, (player_new_shard_id, Instant::now()));

            // on recalcule pour tous les player de la shard si ils sont dans des marge entre les 4 nouvelles shard
            // pas besoin de se retirer soit meme (shard) car il le faut aussi !

            let near_shards = current_node.shards_near(player_pos, self.margin);
            push_handoffs(player_id, near_shards, &mut outgoing_packets);
        }


        for (player_id, pos) in player_loss {

            let old_shard_id = shard_id;
            let new_shard_id = self.quad_tree.shard_id_for(pos)?;

            outgoing_packets.append(
                self.apply_client_cross(
                    player_id,
                    old_shard_id,
                    new_shard_id,
                    pos
                ).as_mut()
            );

            outgoing_packets.push(fast_handoff_req(player_id, old_shard_id));
        }

        // /!\ gerer aussi les ghosts de l'ancien chard (parent)

        // je redefinis current_node en non mut sinon le Borrow checker va me gronder ...
        let current_node = self.quad_tree.get_shard_by_id(shard_id)?;

        for player in self.ghost_client.entry(shard_id).or_default().drain(..) {
            // pour chaque ghost de la shard qui a été subdivisé,
            // on les met en ghost dans les nouvelles shard qui sont dans la marge du player,
            // on doit donc recup la position du player en allant la chercher dans le quad_tree
            let player_shard_id = self.client_to_shards.get(&player)?.0;
            let player_pos = self
                .quad_tree
                .get_shard_by_id(player_shard_id)?
                .players
                .get(&player)?
                .clone();
            let near_shards = current_node.shards_near(player_pos, self.margin);
            push_handoffs(player, near_shards, &mut outgoing_packets);
        }

        let shutdown_packet = ShutdownServerOnEmpty {
            shard_id: shard_id.into(),
        };

        outgoing_packets.push((PeerType::Broker, shutdown_packet.to_bytes()));

        Some(outgoing_packets)

         */
        None
    }

    pub fn merge_request(&mut self, shard_id: ShardId) -> Option<Vec<(PeerType, Bytes)>> {
        /*
        let mut outgoing_packets: Vec<(PeerType, Bytes)> = Vec::new();

        let current_node = self.quad_tree.get_shard_by_id_mut(shard_id)?;

        self.shard_waiting_for_merge.insert(shard_id);

        let spawn_server = SpawnServer {
            shard_id: shard_id.into(),
        };

        outgoing_packets.push((PeerType::Orchestrator, spawn_server.to_bytes()));

        Some(outgoing_packets)

         */
        None
    }

    pub fn merge_node(&mut self, shard_id: ShardId) -> Option<Vec<(PeerType, Bytes)>> {
        /*
        let mut outgoing_packets: Vec<(PeerType, Bytes)> = Vec::new();

        let current_node = self.quad_tree.get_shard_by_id_mut(shard_id)?;

        let players = current_node.merge_quad_tree(shard_id);

        for (player_id,old_shard_id) in players.0 {
            self.client_waiting_for_crossing.insert(player_id, (old_shard_id,shard_id));
            self.client_to_shards
                .insert(player_id, (shard_id, Instant::now()));
            outgoing_packets.push(fast_handoff_req(player_id, shard_id));
        }

        for old_shard_id in &players.1 {
            if let Some(ghosts) = self.ghost_client.get_mut(old_shard_id) {
                for ghost in ghosts.drain(..) {
                    let ghost_current_shar_id = self.client_to_shards.get(&ghost)?.clone();
                    if ghost_current_shar_id.0 != shard_id {
                        outgoing_packets.push(fast_handoff_req(ghost, shard_id));
                    }
                }
            }
        }

        for old_shard_id in &players.1 {
            let shutdown_packet = ShutdownServerOnEmpty {
                shard_id: (*old_shard_id).into(),
            };
            outgoing_packets.push((PeerType::Broker, shutdown_packet.to_bytes()));
        }

        Some(outgoing_packets)

         */
        None
    }

    pub fn apply_client_cross(
        &mut self,
        client_id: ClientId,
        old_shard_id: ShardId,
        new_shard_id: ShardId,
        new_pos: Vec2<f32>,
    ) -> Vec<(PeerType, Bytes)> {
        /*
        let mut outgoing_packets: Vec<(PeerType, Bytes)> = Vec::new();

        self.client_waiting_for_crossing.remove(&client_id);

        if let Some(ghost_in_shards) = self.ghost_client.get_mut(&new_shard_id)
            && ghost_in_shards.contains(&client_id)
        {
            // Le client est déjà en ghost dans le nouveau shard !!!
            ghost_in_shards.retain(|&s| s != client_id);

            self.ghost_client
                .entry(old_shard_id)
                .or_default()
                .push(client_id);

            let handoff_complete = HandoffComplete {
                new_shard_id: new_shard_id.into(),
                old_shard_id: old_shard_id.into(),
                entity_id: client_id.into(),
                pos: new_pos,
            };

            outgoing_packets.push((PeerType::Broker, handoff_complete.to_bytes()));
        } else {
            // Le client n'est pas en ghost dans le nouveau shard :-(
            // ducoup, on attend que le serv rep par un handoffAccept
            // normalement, il a deja été ping par un handoffRequest quand player est entré dans margin

            self.client_waiting_for_crossing
                .insert(client_id, (old_shard_id, new_shard_id));
        }

        if self.quad_tree.insert_player(client_id, new_pos).is_none() {
            println!(
                "error : failed to insert player position in quad_tree for client_id {:?} and shard_id {:?} (apply_client_cross)",
                client_id, new_shard_id
            );
        };

        self.quad_tree.remove_player(client_id, old_shard_id);
        self.client_to_shards.insert(client_id, (new_shard_id, Instant::now()));

        outgoing_packets

         */
        Vec::new()
    }


}

pub fn fast_handoff_req(client_id: ClientId, new_shard_id: ShardId) -> (PeerType, Bytes) {
    let handoff_complete = HandoffRequest {
        shard_id: new_shard_id.into(),
        entity_id: client_id.into(),
    };
    (PeerType::Broker, handoff_complete.to_bytes())
}
