mod quad_tree;
mod shard_id;

use ahash::AHashMap;
use mathtools::Vec2;
use bytes::{Buf, Bytes};
use quad_tree::{QuadTree, Rect};
use crate::shard_id::{ShardId};

fn main() {
    println!("Hello, world!");
}

fn handle_data(raw_bytes: Bytes, spatial_service: &mut SpatialService) {
    match SpatialServerPacket::try_from_bytes(raw_bytes) {
        Some(SpatialServerPacket::Position(update)) => {
            spatial_service.process_update(update);
        }

        Some(SpatialServerPacket::Subdivide(update)) => {
            spatial_service.process_subdivide(update);
        }

        Some(SpatialServerPacket::PlayerJoin(update)) => {
            spatial_service.process_player_join(update);
        }

        None => {
            eprintln!("Paquet binaire invalide ou Tag inconnu reçu.");
        },
    }
}

pub struct SpatialService {
    pub quad_tree: QuadTree,
    pub client_to_shards: AHashMap<u32, ShardId>,
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

    pub fn process_update(&mut self, update_data: PositionUpdate)->Option<()>{
        update_data.client_id;
        update_data.pos;

        let new_shard_id: ShardId = self.quad_tree.insert_player(update_data.client_id,update_data.pos)?;
        let old_shard_id = self.client_to_shards.insert(update_data.client_id, new_shard_id);

        if !old_shard_id.is_none() {
            self.quad_tree.remove_player(update_data.client_id, old_shard_id?)?;
            // TODO : faire les modif du pub sub pour old_shard_id
        }
        if old_shard_id.is_none() || old_shard_id? != new_shard_id {
            // TODO : faire les modif du pub sub pour new_shard_id
        }
        Some(())
    }

    pub fn process_subdivide(&mut self, update_data: SubdivideUpdate) ->Option<()>{

        let shard_id : ShardId = update_data.shard_id.into();

        let mut current_node = &mut self.quad_tree;

        for quadrant in shard_id.id_to_path() {
            current_node = current_node.get_shard(quadrant)?;
        }

        let new_sub:Vec<(u32, ShardId)> = current_node.subdivide_quad_tree();
        let mut old_sub:Vec<(u32, ShardId)> = Vec::new();

        for (player_id, shard_id) in new_sub {
            let old_shard = self.client_to_shards.insert(player_id, shard_id)?;
            old_sub.push((player_id,old_shard));
        };

        //TODO faire les modif du pub sub enregistrées dans new_sub et old_sub

        Some(())
    }

    pub fn process_player_join(&mut self, update_data: PlayerJoinUpdate) -> Option<()>{

        update_data.client_id;
        update_data.pos;

        let new_shard_id: ShardId = self.quad_tree.insert_player(update_data.client_id,update_data.pos)?;

        self.client_to_shards.insert(update_data.client_id,new_shard_id);

        // Todo : faire les modif du pub sub

        Some(())
    }
}

// a mettre dans shared -----------------------------------------

pub struct PositionUpdate {
    pub client_id: u32,
    pub pos: Vec2<f32>,
}

pub struct SubdivideUpdate {
    pub shard_id: u32,
}

pub struct PlayerJoinUpdate {
    pub client_id: u32,
    pub pos: Vec2<f32>,
}

pub enum SpatialServerPacket {
    Position(PositionUpdate),
    Subdivide(SubdivideUpdate),
    PlayerJoin(PlayerJoinUpdate),
}

impl SpatialServerPacket {
    pub fn try_from_bytes(data: Bytes) -> Option<Self> {
        if data.is_empty() {
            return None;
        }
        let tag = data[0];
        match tag {
            PositionUpdate::TAG => {
                PositionUpdate::try_from_bytes(data).map(SpatialServerPacket::Position)
            }
            SubdivideUpdate::TAG => {
                SubdivideUpdate::try_from_bytes(data).map(SpatialServerPacket::Subdivide)
            }
            PlayerJoinUpdate::TAG => {
                PlayerJoinUpdate::try_from_bytes(data).map(SpatialServerPacket::PlayerJoin)
            }
            _ => None,
        }
    }
}

pub trait SpatialServerBinaryPacket: Sized {
    const TAG: u8;
    const PACKET_SIZE: usize;

    fn parse_payload(data: &mut Bytes) -> Option<Self>;
    fn write_payload(&self, buf: &mut Vec<u8>);

    fn try_from_bytes(mut data: Bytes) -> Option<Self> {
        if data.len() < Self::PACKET_SIZE {
            return None;
        }

        if data.get_u8() != Self::TAG {
            return None;
        }

        Self::parse_payload(&mut data)
    }

    fn to_bytes(&self) -> Bytes {
        let mut buf = Vec::with_capacity(Self::PACKET_SIZE);
        buf.push(Self::TAG);
        self.write_payload(&mut buf);
        Bytes::from(buf)
    }
}

impl SpatialServerBinaryPacket for PositionUpdate {
    const TAG: u8 = 0x10;
    const PACKET_SIZE: usize = 13;

    #[inline]
    fn parse_payload(data: &mut Bytes) -> Option<Self> {
        let client_id = data.get_u32_le();
        let x = data.get_f32_le();
        let y = data.get_f32_le();

        Some(Self {
            client_id,
            pos: Vec2::new(x, y),
        })
    }

    #[inline]
    fn write_payload(&self, buf: &mut Vec<u8>) {
        buf.extend_from_slice(&self.client_id.to_le_bytes());
        buf.extend_from_slice(&self.pos.x.to_le_bytes());
        buf.extend_from_slice(&self.pos.y.to_le_bytes());
    }
}

impl SpatialServerBinaryPacket for SubdivideUpdate {
    const TAG: u8 = 0x11;
    const PACKET_SIZE: usize = 5;

    #[inline]
    fn parse_payload(data: &mut Bytes) -> Option<Self> {
        Some(Self {
            shard_id: data.get_u32_le(),
        })
    }

    #[inline]
    fn write_payload(&self, buf: &mut Vec<u8>) {
        buf.extend_from_slice(&self.shard_id.to_le_bytes());
    }
}

impl SpatialServerBinaryPacket for PlayerJoinUpdate {
    const TAG: u8 = 0x12;
    const PACKET_SIZE: usize = 13;

    #[inline]
    fn parse_payload(data: &mut Bytes) -> Option<Self> {
        let client_id = data.get_u32_le();
        let x = data.get_f32_le();
        let y = data.get_f32_le();

        Some(Self {
            client_id,
            pos: Vec2::new(x, y),
        })
    }

    #[inline]
    fn write_payload(&self, buf: &mut Vec<u8>) {
        buf.extend_from_slice(&self.client_id.to_le_bytes());
        buf.extend_from_slice(&self.pos.x.to_le_bytes());
        buf.extend_from_slice(&self.pos.y.to_le_bytes());
    }
}