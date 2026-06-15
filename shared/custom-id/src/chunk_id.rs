use std::fmt;
use mathtools::Vec2;

use crate::custom_id::{CustomId, IdType};

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct ChunkId(CustomId);

impl ChunkId {
    pub const CHUNK_SIZE: f32 = 500.0;
    const COORD_BITS: u8 = 14;
    const COORD_MASK: u32 = 0x3FFF; // 14 bits
    const OFFSET: i32 = 8192;

    #[inline]
    pub fn new(x: i32, y: i32) -> Result<Self, &'static str> {
        let x_adj = x + Self::OFFSET;
        let y_adj = y + Self::OFFSET;

        if x_adj < 0 || x_adj > Self::COORD_MASK as i32 || y_adj < 0 || y_adj > Self::COORD_MASK as i32 {
            return Err("Coordonnées du chunk hors limites (-8192 à 8191)");
        }

        let val = ((x_adj as u32) << Self::COORD_BITS) | (y_adj as u32);
        Ok(Self(CustomId::new_unchecked(IdType::Chunk, val)))
    }

    pub fn from_position(pos: Vec2<f32>) -> Result<Self, &'static str> {
        let x = (pos.x / Self::CHUNK_SIZE).floor() as i32;
        let y = (pos.y / Self::CHUNK_SIZE).floor() as i32;
        Self::new(x, y)
    }

    #[inline]
    pub fn x(&self) -> i32 {
        let val = self.0.value();
        let x_adj = (val >> Self::COORD_BITS) & Self::COORD_MASK;
        (x_adj as i32) - Self::OFFSET
    }

    #[inline]
    pub fn y(&self) -> i32 {
        let val = self.0.value();
        let y_adj = val & Self::COORD_MASK;
        (y_adj as i32) - Self::OFFSET
    }

    pub fn get_surrounding_chunks(&self) -> [ChunkId; 9] {
        let cx = self.x();
        let cy = self.y();

        [
            Self::new(cx - 1, cy + 1).unwrap(),
            Self::new(cx, cy + 1).unwrap(),
            Self::new(cx + 1, cy + 1).unwrap(),
            Self::new(cx - 1, cy).unwrap(),
            Self::new(cx, cy).unwrap(),
            Self::new(cx + 1, cy).unwrap(),
            Self::new(cx - 1, cy - 1).unwrap(),
            Self::new(cx, cy - 1).unwrap(),
            Self::new(cx + 1, cy - 1).unwrap(),
        ]
    }
}

impl TryFrom<CustomId> for ChunkId {
    type Error = &'static str;

    fn try_from(id: CustomId) -> Result<Self, Self::Error> {
        if id.id_type()? != IdType::Chunk {
            return Err("Le CustomId fourni n'est pas du type Chunk");
        }
        Ok(Self(id))
    }
}

impl From<ChunkId> for CustomId {
    #[inline]
    fn from(chunk_id: ChunkId) -> Self {
        chunk_id.0
    }
}

impl From<ChunkId> for u32 {
    #[inline]
    fn from(value: ChunkId) -> Self {
        value.0.as_u32()
    }
}

impl fmt::Debug for ChunkId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ChunkId(X: {}, Y: {})", self.x(), self.y())
    }
}