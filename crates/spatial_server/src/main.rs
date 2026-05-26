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
        }

        _ => {
            eprintln!("Paquet reçu mais pas encore géré dans le SpatialService.");
        }
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

pub trait BinaryField {
    const SIZE: usize;
    fn read_from(data: &mut Bytes) -> Self;
    fn write_to(&self, buf: &mut Vec<u8>);
}

impl BinaryField for u32 {
    const SIZE: usize = 4;

    #[inline]
    fn read_from(data: &mut Bytes) -> Self {
        data.get_u32_le()
    }

    #[inline]
    fn write_to(&self, buf: &mut Vec<u8>) {
        buf.extend_from_slice(&self.to_le_bytes());
    }
}

impl BinaryField for Vec2<f32> {
    const SIZE: usize = 8;

    #[inline]
    fn read_from(data: &mut Bytes) -> Self {
        Vec2::new(data.get_f32_le(), data.get_f32_le())
    }

    #[inline]
    fn write_to(&self, buf: &mut Vec<u8>) {
        buf.extend_from_slice(&self.x.to_le_bytes());
        buf.extend_from_slice(&self.y.to_le_bytes());
    }
}

macro_rules! define_packet {
    (
        // Syntaxe attendue : struct NomDuPaquet(TAG) { champ: Type, ... }
        $struct_name:ident($tag:expr) {
            $( $field_name:ident : $field_type:ty ),* $(,)?
        }
    ) => {
        // 1. Génération de la structure
        pub struct $struct_name {
            $( pub $field_name: $field_type, )*
        }

        // 2. Implémentation du trait principal
        impl SpatialServerBinaryPacket for $struct_name {
            const TAG: u8 = $tag;

            // Calcul de la taille : 1 octet (TAG) + somme des tailles des champs
            const PACKET_SIZE: usize = 1 $( + <$field_type as BinaryField>::SIZE )*;

            #[inline]
            fn parse_payload(data: &mut Bytes) -> Option<Self> {
                Some(Self {
                    $( $field_name: <$field_type as BinaryField>::read_from(data), )*
                })
            }

            #[inline]
            fn write_payload(&self, buf: &mut Vec<u8>) {
                $( self.$field_name.write_to(buf); )*
            }
        }
    };
}

macro_rules! define_packet_router {
    (
        $vis:vis enum $enum_name:ident {
            // Accepte une liste de variantes sous la forme `NomVariante(TypeStructure)`
            $( $variant:ident($packet_type:ty) ),* $(,)?
        }
    ) => {
        // 1. Génération de l'Enum
        $vis enum $enum_name {
            $( $variant($packet_type), )*
        }

        // 2. Génération de l'implémentation de routage
        impl $enum_name {
            pub fn try_from_bytes(data: Bytes) -> Option<Self> {
                if data.is_empty() {
                    return None;
                }

                let tag = data[0];

                match tag {
                    $(
                        // Utilise les constantes associées du trait BinaryPacket
                        <$packet_type>::TAG => {
                            <$packet_type>::try_from_bytes(data).map(Self::$variant)
                        }
                    )*
                    _ => None,
                }
            }
        }
    };
}

define_packet_router! {
    pub enum SpatialServerPacket {
        Subscribe(Subscribe),
        Unsubscribe(Unsubscribe),
        Position(PositionUpdate),
        Subdivide(SubdivideUpdate),
        PlayerJoin(PlayerJoinUpdate),
    }
}

define_packet! {
    Subscribe(0x01) {
        client_id: u32,
        shard_id: u32,
    }
}

define_packet! {
    Unsubscribe(0x02) {
        client_id: u32,
        shard_id: u32,
    }
}

define_packet! {
    PositionUpdate(0x10) {
        client_id: u32,
        pos: Vec2<f32>,
    }
}

define_packet!{
    SubdivideUpdate(0x11) {
        shard_id: u32,
    }
}

define_packet!{
    PlayerJoinUpdate(0x12) {
        client_id: u32,
        pos: Vec2<f32>,
    }
}
