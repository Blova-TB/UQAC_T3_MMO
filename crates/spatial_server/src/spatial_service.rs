use ahash::AHashMap;
use bytes::Bytes;
use shared::models::{CustomId, PlayerJoinUpdate, PositionUpdate, ServerBinaryPacket, ShutdownServer, SpawnServer, SubdivideUpdate, Subscribe, Unsubscribe};
use crate::network::PeerType;
use crate::quad_tree::{QuadTree, Rect};
use crate::shard_id::{Quadrant, ShardId};
use crate::client_id::ClientId;

pub struct SpatialService {
    pub quad_tree: QuadTree,
    pub client_to_shards: AHashMap<ClientId, ShardId>,
    pub margin: f32,
}

impl SpatialService {
    pub fn new(bounds: Rect, max_depth: u8, margin: f32) -> Self {
        Self {
            quad_tree: QuadTree::new(bounds, 0, max_depth, ShardId::ROOT),
            client_to_shards: AHashMap::new(),
            margin,
        }
    }

    pub fn process_update(&mut self, update_data: PositionUpdate) -> Option<Vec<(PeerType, Bytes)>> {
        let mut outgoing_packets: Vec<(PeerType, Bytes)> = Vec::new();

        // On convertit le CustomId du paquet en ClientId.
        // Si ça échoue (mauvais type), on retourne None silencieusement pour ignorer le paquet malveillant/erroné.
        let client_id = ClientId::try_from(update_data.client_id).ok()?;

        let new_shard_id: ShardId = self.quad_tree.insert_player(client_id, update_data.pos)?;
        let old_shard_id = self.client_to_shards.insert(client_id, new_shard_id);

        if let Some(old_shard) = old_shard_id {
            self.quad_tree.remove_player(client_id, old_shard)?;
            let unsub = Unsubscribe {
                client_id: client_id.into(), // Conversion automatique en CustomId pour le paquet réseau
                topic_id: old_shard.into(),
            };
            outgoing_packets.push((PeerType::Broker, unsub.to_bytes()));
        }

        if old_shard_id.is_none() || old_shard_id.unwrap() != new_shard_id {
            let sub = Subscribe {
                client_id: client_id.into(), // Idem ici
                topic_id: new_shard_id.into(),
            };
            outgoing_packets.push((PeerType::Broker, sub.to_bytes()));
        }

        Some(outgoing_packets)
    }

    pub fn process_subdivide(&mut self, update_data: SubdivideUpdate) -> Option<Vec<(PeerType, Bytes)>> {
        let mut outgoing_packets: Vec<(PeerType, Bytes)> = Vec::new();

        // update_data.shard_id est sûrement un u32 ou un CustomId selon ton modèle.
        // On le convertit en ShardId pour naviguer dans l'arbre.
        let shard_id = ShardId::try_from(update_data.shard_id).ok()?;

        let mut current_node = &mut self.quad_tree;

        for quadrant in shard_id.id_to_path() {
            current_node = current_node.get_shard(quadrant)?;
        }

        if current_node.depth >= current_node.max_depth {
            println!("Shard {:?} is already at max depth, cannot subdivide further.", shard_id);
            return None;
        }

        let new_sub: Vec<(ClientId, ShardId)> = current_node.subdivide_quad_tree();
        let mut old_sub: Vec<(ClientId, ShardId)> = Vec::new();

        for &(player_id, new_shard) in &new_sub {
            if let Some(old_shard) = self.client_to_shards.insert(player_id, new_shard) {
                old_sub.push((player_id, old_shard));
            }
        }

        for &(player_id, old_shard) in &old_sub {
            let unsub = Unsubscribe {
                client_id: player_id.into(),
                topic_id: old_shard.into(),
            };
            outgoing_packets.push((PeerType::Broker, unsub.to_bytes()));
        }

        for &(player_id, new_shard) in &new_sub {
            let sub = Subscribe {
                client_id: player_id.into(),
                topic_id: new_shard.into(),
            };
            outgoing_packets.push((PeerType::Broker, sub.to_bytes()));
        }

        for quadr in Quadrant::get_all() {
            let Rect { min, max } = quadr.get_bound_from_parent(&current_node.bounds);
            let spawn_server = SpawnServer {
                shard_id: shard_id.new_id_for_child(quadr).into(),
                pos_max: max,
                pos_min: min,
            };
            outgoing_packets.push((PeerType::Orchestrator, spawn_server.to_bytes()));
        }

        let shutdown_server = ShutdownServer {
            shard_id: shard_id.into(),
        };
        outgoing_packets.push((PeerType::Orchestrator, shutdown_server.to_bytes()));

        Some(outgoing_packets)
    }

    pub fn process_player_join(&mut self, update_data: PlayerJoinUpdate) -> Option<Vec<(PeerType, Bytes)>> {
        let mut outgoing_packets: Vec<(PeerType, Bytes)> = Vec::new();

        let client_id = ClientId::try_from(update_data.client_id).ok()?;
        let new_shard_id: ShardId = self.quad_tree.insert_player(client_id, update_data.pos)?;

        self.client_to_shards.insert(client_id, new_shard_id);

        // Todo : faire les modif du pub sub

        Some(outgoing_packets)
    }
}