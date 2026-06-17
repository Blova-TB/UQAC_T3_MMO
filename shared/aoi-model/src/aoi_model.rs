use mathtools::Vec2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AoiMode {
    NormalRange,
    ExtendedRange,
}

impl AoiMode {
    #[inline]
    pub const fn size(self) -> i32 {
        match self {
            Self::NormalRange => 4,
            Self::ExtendedRange => 6,
        }
    }

    /// Offset entre le centre (top-left center chunk) et le "top-left" de toute la zone
    #[inline]
    pub const fn origin_offset(self) -> i32 {
        (self.size() / 2) - 1
    }
    
    #[inline]
    pub const fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::NormalRange),
            1 => Some(Self::ExtendedRange),
            _ => None,
        }
    }
    
    pub fn to_u8(self) -> u8 {
        match self {
            Self::NormalRange => 0,
            Self::ExtendedRange => 1,
        }
    }
    
    #[inline]
    pub const fn default() -> Self {
        Self::NormalRange
    }
}

/// min inclusif, max exclusif.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AoiBounds {
    pub min: Vec2<i32>,
    pub max: Vec2<i32>,
}

impl AoiBounds {
    pub fn from_center_pos(center_pos: Vec2<i32>, mode: AoiMode) -> Self {
        let offset = mode.origin_offset();
        let size = mode.size();
        Self {
            min: Vec2 {
                x: center_pos.x - offset,
                y: center_pos.y - offset,
            },
            max: Vec2 {
                x: center_pos.x - offset + size,
                y: center_pos.y - offset + size,
            },
        }
    }

    #[inline]
    pub fn contains(&self, chunk: &Vec2<i32>) -> bool {
        chunk.x >= self.min.x
            && chunk.x < self.max.x
            && chunk.y >= self.min.y
            && chunk.y < self.max.y
    }

    pub fn iter_chunks(&self) -> impl Iterator<Item = Vec2<i32>> + '_ {
        (self.min.x..self.max.x).flat_map(move |x| {
            (self.min.y..self.max.y).map(move |y| Vec2 { x, y })
        })
    }
}

pub struct AoiDelta {
    pub added: Vec<Vec2<i32>>,
    pub removed: Vec<Vec2<i32>>,
}

pub fn calculate_aoi_delta(old_bounds: AoiBounds, new_bounds: AoiBounds) -> AoiDelta {
    if old_bounds == new_bounds {
        return AoiDelta { added: Vec::new(), removed: Vec::new() };
    }

    let max_capacity = (new_bounds.max.x - new_bounds.min.x) * (new_bounds.max.y - new_bounds.min.y);
    let mut added = Vec::with_capacity(max_capacity as usize);
    let mut removed = Vec::with_capacity(max_capacity as usize);

    for chunk in new_bounds.iter_chunks() {
        if !old_bounds.contains(&chunk) {
            added.push(chunk);
        }
    }

    for chunk in old_bounds.iter_chunks() {
        if !new_bounds.contains(&chunk) {
            removed.push(chunk);
        }
    }

    AoiDelta { added, removed }
}