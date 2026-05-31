use ahash::{AHashMap, AHashSet};
use bytes::Bytes;
use mathtools::Vec2;
use shared::models::{HandoffAccept, HandoffComplete, HandoffDrop, HandoffRequest, PlayerJoinUpdate, PositionUpdate, ServerBinaryPacket, ServerHandShake, ServerSpawned, SpawnServer};
use crate::network::PeerType;
use crate::quad_tree::{QuadTree, Rect};
use crate::shard_id::{Quadrant, ShardId};
use crate::client_id::ClientId;

pub struct SpatialService {
    pub quad_tree: QuadTree,
    pub client_to_shards: AHashMap<ClientId, ShardId>,
    pub ghost_client: AHashMap<ShardId,Vec<ClientId>>, // shard_id -> client_id: is replicate in ghost on this serv ? (if exist alors le shard est pret à recevoir l'autorité)
    pub client_waiting_for_crossing: AHashMap<ClientId,ShardId>, // client_id -> (shard_id) : client pas encore repliqué en ghost mais qui a deja cross.
    pub shard_waiting_for_subdivide: AHashMap<ShardId,Vec<(ShardId,bool)>>, // shard_id : shard qui a demandé une subdivision et qui attend que tous les clients soient en ghost pour subdiviser
    pub margin: f32,
    pub occupation_to_subdivide: f32,
    pub occupation_to_merge: f32,
    pub time_for_merge_after_subdivide: f32,
}

impl SpatialService {
    pub(crate) fn process_player_join(&self, p0: PlayerJoinUpdate) -> Option<Vec<(PeerType, Bytes)>> {
        todo!()
    }
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
            shard_waiting_for_subdivide: AHashMap::new(),
            margin,
            occupation_to_subdivide,
            occupation_to_merge,
            time_for_merge_after_subdivide,
        }
    }

    pub fn process_position_update(&mut self, update_data: PositionUpdate) -> Option<Vec<(PeerType, Bytes)>> {
        let mut outgoing_packets: Vec<(PeerType, Bytes)> = Vec::new();

        let client_id = ClientId::try_from(update_data.client_id).ok()?;

        let new_pos = update_data.pos;
        let new_shard_id = self.quad_tree.insert_player(client_id, new_pos)?;

        let old_shard_id_opt = self.client_to_shards.insert(client_id, new_shard_id);
        let old_pos_opt : Option<Vec2<f32>>;

        let Some(old_shard_id) = old_shard_id_opt else {
            println!("error : NOUVEAU JOUEUR dans le process_update ?????");
            return None;
        };

        if let Some(old_node) = self.quad_tree.get_shard_by_id_mut(old_shard_id){
            old_pos_opt = old_node.players.get(&client_id).copied();
        } else {
            println!(
                "error : ANCIEN SHARD INEXISTANT dans le process_update ????? (client_id: {:?}, old_shard_id: {:?})",
                client_id, old_shard_id
            );
            // todo:
            // probablement un cas où le joueur etait dans une shard qui a été subdivisé ou fusionné en meme temps qu'il a change de shard.
            return None;
        }

        let Some(old_pos) = old_pos_opt else {
            println!(
                "error : ANCIEN JOUEUR INEXISTANT dans le process_update ????? (client_id: {:?}, old_shard_id: {:?})",
                client_id, old_shard_id
            );
            return None;
        };

        if let Some(old_shard_id) = old_shard_id_opt {
            if old_shard_id != new_shard_id {
                self.quad_tree.remove_player(client_id, old_shard_id)?;
                println!(
                    "🚨 CROSSING ALERT : Joueur {:?} a changé de shard ({:?} -> {:?})",
                    client_id, old_shard_id, new_shard_id
                );

                self.client_waiting_for_crossing.remove(&client_id);

                if let Some(ghost_in_shards) = self.ghost_client.get_mut(&new_shard_id)
                    && ghost_in_shards.contains(&client_id){

                    // Le client est déjà en ghost dans le nouveau shard !!!
                    ghost_in_shards.retain(|&s| s != client_id);

                    self.ghost_client.entry(old_shard_id).or_default().push(client_id);

                    let handoff_complete = HandoffComplete {
                        shard_id: new_shard_id.into(),
                        entity_id: client_id.into(),
                    };

                    outgoing_packets.push((PeerType::Broker, handoff_complete.to_bytes()));
                }else{

                    // Le client n'est pas en ghost dans le nouveau shard :-(
                    // ducoup on attend que le serv rep par un handoffAccept
                    // normalement il a deja été ping par un handoffRequest quand player est entré dans margin

                    self.client_waiting_for_crossing.insert(client_id, new_shard_id);
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
        for &left_shard in past_visible_shards.difference(&current_visible_shards) {
            let unsub = HandoffDrop {
                shard_id: left_shard.into(),
                entity_id: client_id.into(),
            };
            outgoing_packets.push((PeerType::Broker, unsub.to_bytes()));
        }

        // Subscribe
        for &entered_shard in current_visible_shards.difference(&past_visible_shards) {

            let sub = HandoffRequest {
                shard_id: entered_shard.into(),
                entity_id: client_id.into(),
            };

            outgoing_packets.push((PeerType::Broker, sub.to_bytes()));
        }

        Some(outgoing_packets)
    }

    pub fn process_handoff_accept(&mut self, update_data: HandoffAccept) -> Option<Vec<(PeerType, Bytes)>> {
        let client_id = ClientId::try_from(update_data.entity_id).ok()?;
        let shard_id = ShardId::try_from(update_data.shard_id).ok()?;


        if let Some(waiting_shard) = self.client_waiting_for_crossing.get(&client_id) {
            if *waiting_shard == shard_id { // si le client attendait de cross dans cette shard
                self.client_waiting_for_crossing.remove(&client_id);

                let handoff_complete = HandoffComplete {
                    shard_id: shard_id.into(),
                    entity_id: client_id.into(),
                };

                return Some(vec![(PeerType::Broker, handoff_complete.to_bytes())]);
            }
        }

        self.ghost_client.entry(shard_id).or_default().push(client_id);
        None
    }

    pub fn process_server_handshake(&mut self, update_data: ServerHandShake) -> Option<Vec<(PeerType,Bytes)>>{

        let shard_id : ShardId = ShardId::try_from(update_data.shard_id).ok()?;

        let current_node = self.quad_tree.get_shard_by_id_mut(shard_id)?;

        current_node.server_occupation = Option::from(update_data.occupancy);

        if update_data.occupancy > self.occupation_to_subdivide {
            if current_node.depth >= current_node.max_depth {
                print!("Shard {:?} is already at max depth, cannot subdivide further.", shard_id);
                return None;
            }
            self.subdivide_request(shard_id);
        }

        None
    }

    pub fn subdivide_request(&mut self, shard_id: ShardId) ->Option<Vec<(PeerType, Bytes)>>{

        let mut outgoing_packets : Vec<(PeerType,Bytes)> = Vec::new();

        let current_bounds = &self.quad_tree.get_shard_by_id_mut(shard_id)?.bounds;

        for quads in Quadrant::get_all() {
            let Rect { min, max } = quads.get_bound_from_parent(current_bounds);
            let spawn_server = SpawnServer {
                shard_id: shard_id.new_id_for_child(quads).into(),
                pos_max: max,
                pos_min: min,
            };

            outgoing_packets.push((PeerType::Orchestrator, spawn_server.to_bytes()));
        }

        self.shard_waiting_for_subdivide.insert(shard_id, Vec::from(
            [
                (shard_id.new_id_for_child(Quadrant::TopLeft), false),
                (shard_id.new_id_for_child(Quadrant::TopRight), false),
                (shard_id.new_id_for_child(Quadrant::BottomLeft), false),
                (shard_id.new_id_for_child(Quadrant::BottomRight), false),
            ]
        ));

        Some(outgoing_packets)

    }

    pub fn process_server_spawned(&mut self, update_data: ServerSpawned) -> Option<Vec<(PeerType,Bytes)>>{

        let shard_id : ShardId = ShardId::try_from(update_data.shard_id).ok()?;

        let Some(parent_shard_id) = shard_id.get_parent_shard_id() else {
            println!("WHHHAAATT YOU ARE THE ROOT SHARD ??? SO COOOOL !!! but ... no (Error : process_server_spawned)");
            return None;
        };

        if let Some(children) = self.shard_waiting_for_subdivide.get_mut(&parent_shard_id) {

            if let Some(child) = children.iter_mut().find(|(id, _)| *id == shard_id) {
                child.1 = true;
            }else {
                println!("error : shard_id {:?} not found in waiting_for_subdivide for parent_shard_id {:?} (process_server_spawned)", shard_id, parent_shard_id);
                return None;
            }

            if children.iter().all(|&(_, spawned)| spawned) {

                self.shard_waiting_for_subdivide.remove(&parent_shard_id);

                self.subdivide_node(parent_shard_id);
            }
        }else {
            println!("error : parent_shard_id {:?} not found in waiting_for_subdivide (process_server_spawned)", parent_shard_id);
        }

        None
    }

    pub fn subdivide_node(&mut self, shard_id: ShardId) ->Option<Vec<(PeerType, Bytes)>>{

        let mut outgoing_packets : Vec<(PeerType,Bytes)> = Vec::new();

        let current_node = self.quad_tree.get_shard_by_id_mut(shard_id)?;

        if current_node.depth >= current_node.max_depth {
            println!("Shard {:?} is already at max depth, cannot subdivide further.", shard_id);
            return None;
        }

        let push_handoffs = |entity_id: ClientId, near_shards: Vec<ShardId>, packets: &mut Vec<(PeerType, Bytes)>| {
            for near_shard_id in near_shards {
                let handoff_request = HandoffRequest {
                    shard_id: near_shard_id.into(),
                    entity_id: entity_id.into(),
                };
                packets.push((PeerType::Broker, handoff_request.to_bytes()));
            }
        };

        let players = current_node.subdivide_quad_tree();


        // on prend tous les joueurs et on les met dans self.waiting_for_crossing
        // dé qu'ils sont accepté dans le nouveau shard (handoffAccept), ils changenet de shard

        for (player_id, player_new_shard_id, player_pos) in players {
            self.client_waiting_for_crossing.insert(player_id, player_new_shard_id);

            // on recalcule pour tout les player de la shard si ils sont dans des marge entre les 4 nouvelles shard
            // pas besoin de se retirer soit meme (shard) car il le faut aussi !

            let near_shards = current_node.shards_near(player_pos, self.margin);
            push_handoffs(player_id, near_shards, &mut outgoing_packets);
        }

        // /!\ gerer aussi les ghosts de l'ancien chard (parent)

        // je redefinis current_node en non mut sinon le Borrow checker va me gronder ...
        let current_node = self.quad_tree.get_shard_by_id(shard_id)?;

        for player in self.ghost_client.entry(shard_id).or_default().drain(..) {

            // pour chaque ghost de la shard qui a été subdivisé,
            // on les met en ghost dans les nouvelles shard qui sont dans la marge du player,
            // on doit donc recup la position du player en allant la chercher dans le quad_tree
            let player_shard_id = self.client_to_shards.get(&player)?.clone();
            let player_pos = self.quad_tree.get_shard_by_id(player_shard_id)?.players.get(&player)?.clone();
            let near_shards = current_node.shards_near(player_pos, self.margin);
            push_handoffs(player, near_shards, &mut outgoing_packets);
        }

        Some(outgoing_packets)
    }
}