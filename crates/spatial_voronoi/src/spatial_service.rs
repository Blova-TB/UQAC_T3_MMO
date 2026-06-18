use std::time::Instant;
use crate::client_id::ClientId;
use crate::network::PeerType;
use crate::voronoi::{Voronoi, VoronoiEvent};
use crate::shard_id::{ShardId};
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

        let Some(player) = self.voronoi.remove_player(client_id, shard_id) else {
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

        // 1. Mise à jour de la position et récupération de l'ancienne
        let Some(old_pos) = self.voronoi.update_position_get_old(client_id, Point2D::from(new_pos)) else {
            println!("error : player {:?} not found in voronoi", client_id);
            return None;
        };

        let current_shard = self.voronoi.get_player_shard(client_id)?;

        // 2. Vérification Handoff + Hystérésis Temporelle
        if let Some(optimal_shard) = self.voronoi.check_single_handoff(client_id) {

            // Récupération de l'Instant depuis client_to_shards
            let last_handoff = self.client_to_shards
                .get(&client_id)
                .map(|(_, time)| *time)
                .unwrap_or_else(|| std::time::Instant::now() - std::time::Duration::from_secs(100)); // bypass au premier spawn

            // On vérifie si le délai est écoulé
            if last_handoff.elapsed() > std::time::Duration::from_secs_f32(self.hysteresis_time) {

                println!(
                    "CROSSING ALERT : Joueur {:?} a changé de shard ({:?} -> {:?})",
                    client_id, current_shard, optimal_shard
                );

                // Appliquer les changements d'état
                self.voronoi.update_player_shard(client_id, optimal_shard);

                // Mise à jour simultanée de la shard et du timer d'hystérésis
                self.client_to_shards.insert(client_id, (optimal_shard, std::time::Instant::now()));

                outgoing_packets.append(
                    self.apply_client_cross(
                        client_id,
                        current_shard,
                        optimal_shard,
                        new_pos
                    ).as_mut()
                );
            }
        }

        // 3. Mise à jour de la visibilité (Subscribe / Unsubscribe)
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

        // On vérifie si ce client attendait l'autorisation de ce serveur
        if let Some(&(old_shard_id, waiting_shard)) = self.client_waiting_for_crossing.get(&client_id) {
            if waiting_shard == shard_id {
                self.client_waiting_for_crossing.remove(&client_id);

                // On récupère la position dans Voronoï
                let pos = self.voronoi.get_player_pos(client_id)?;

                let handoff_complete = HandoffComplete {
                    new_shard_id: shard_id.into(),
                    entity_id: client_id.into(),
                    old_shard_id: old_shard_id.into(),
                    pos: pos.into(), // Point2D -> Vec2
                };

                outgoing_packets.push((PeerType::Broker, handoff_complete.to_bytes()));
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

        // On informe le Voronoï que le serveur est prêt.
        // S'il nous renvoie une TopologyUpdate, ça veut dire que l'opération est complétée !
        if let Some(topo_update) = self.voronoi.confirm_server_spawned(shard_id) {
            self.process_topology_commit(&topo_update, &mut outgoing_packets);
        }

        if outgoing_packets.is_empty() { None } else { Some(outgoing_packets) }
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
        } else {
            self.client_waiting_for_crossing.insert(client_id, (old_shard_id, new_shard_id));

            // 🛡️ On envoie la requête EXACTEMENT UNE FOIS ici.
            outgoing_packets.push(fast_handoff_req(client_id, new_shard_id));
        }

        // --- C'EST VORONOÏ LE PATRON MAINTENANT ---
        self.voronoi.update_player_shard(client_id, new_shard_id);
        self.client_to_shards.insert(client_id, (new_shard_id, Instant::now()));

        // 🛡️ FIX ANTI-DOUBLON (Ghost Downgrade) :
        // On force l'ajout de new_shard_id dans l'état ghost pour empêcher
        // process_position_update d'envoyer un second fast_handoff_req au prochain tick.
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
                // Si Voronoï veut diviser, on envoie JUSTE la requête au réseau.
                // On ne touche à aucun joueur.
                VoronoiEvent::SpawnRequests(shards_to_spawn) => {
                    for new_shard in shards_to_spawn {
                        let packet = SpawnServer { shard_id: new_shard.into() };
                        outgoing_packets.push((PeerType::Orchestrator, packet.to_bytes()));
                    }
                }

                // Ce cas arrivera très rarement via le tick normal (uniquement si un fallback immédiat a lieu)
                VoronoiEvent::TopologyCommitted(update) => {
                    self.process_topology_commit(&update, &mut outgoing_packets);
                }
            }
        }

        if outgoing_packets.is_empty() { None } else { Some(outgoing_packets) }
    }

    fn process_topology_commit(&mut self, topo_update: &crate::voronoi::TopologyUpdate, outgoing_packets: &mut Vec<(PeerType, Bytes)>) {
        // 1. Éteindre les vieux serveurs
        for dead_shard in &topo_update.despawned_shards {
            let packet = ShutdownServerOnEmpty { shard_id: (*dead_shard).into() };
            outgoing_packets.push((PeerType::Broker, packet.to_bytes()));
        }

        // 2. Transférer de force les joueurs
        for &(client_id, old_shard, new_shard) in &topo_update.forced_handoffs {

            // 🛡️ FIX ANTI-DROP PRÉMATURÉ :
            // On retire le vieux serveur des ghosts connus. Cela empêche process_position_update
            // d'envoyer un HandoffDrop (qui supprimerait l'entité) avant que le nouveau serveur n'ait pris l'autorité.
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

                // NOTE: Le bloc "NOUVEAU" avec l'envoi de fast_handoff_req qui était ici a été retiré.
                // C'était lui le responsable principal du double-spam qui cassait l'autorité !
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
