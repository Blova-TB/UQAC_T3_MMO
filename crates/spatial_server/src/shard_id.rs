use std::fmt;
use mathtools::Vec2;
use crate::quad_tree::Rect;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Quadrant {
    TopLeft = 0b00,
    TopRight = 0b01,
    BottomLeft = 0b10,
    BottomRight = 0b11,
}

impl Quadrant {
    pub fn get_all() -> [Quadrant; 4] {
        [Quadrant::TopLeft, Quadrant::TopRight, Quadrant::BottomLeft, Quadrant::BottomRight]
    }
    
    pub fn get_bound_from_parent(&self, parent_bounds: &Rect) -> Rect {
        let center = parent_bounds.center();
        match self {
            Quadrant::TopLeft => Rect { min: Vec2 { x: parent_bounds.min.x, y: center.y }, max: Vec2 { x: center.x, y: parent_bounds.max.y } },
            Quadrant::TopRight => Rect { min: center, max: parent_bounds.max },
            Quadrant::BottomLeft => Rect { min: parent_bounds.min, max: center },
            Quadrant::BottomRight => Rect { min: Vec2 { x: center.x, y: parent_bounds.min.y }, max: Vec2 { x: parent_bounds.max.x, y: center.y } },
        }
    }
}


#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct ShardId(pub u32);

impl ShardId {

    const DEPTH_MASK: u32 = 0xF000_0000;
    const DEPTH_SHIFT: u8 = 28;
    pub const MAX_DEPTH: u8 = 14;
    pub const ROOT: Self = Self(0);

    #[inline]
    pub fn depth(self) -> u8 {
        (self.0 >> Self::DEPTH_SHIFT) as u8
    }

    pub fn new_id_for_child(self, quadrant: Quadrant) -> Self {
        let current_depth = self.depth();
        assert!(
            current_depth < Self::MAX_DEPTH,
            "Profondeur maximale du QuadTree atteinte (14)"
        );

        let new_depth = current_depth + 1;

        // Màj depth
        let mut new_id = self.0 & !Self::DEPTH_MASK;
        new_id |= (new_depth as u32) << Self::DEPTH_SHIFT;

        // add quadrant
        let shift = Self::DEPTH_SHIFT - (2 * new_depth);
        new_id |= (quadrant as u32) << shift;

        Self(new_id)
    }

    pub fn id_to_path(self) -> Vec<Quadrant> {
        let depth = self.depth();
        let mut result = Vec::with_capacity(depth as usize);

        for i in 1..=depth {
            let shift = Self::DEPTH_SHIFT - (2 * i);
            let quad_bits = (self.0 >> shift) & 0b11;

            let quad = match quad_bits {
                0b00 => Quadrant::TopLeft,
                0b01 => Quadrant::TopRight,
                0b10 => Quadrant::BottomRight,
                0b11 => Quadrant::BottomLeft,
                _ => unreachable!(),
            };
            result.push(quad);
        }

        result
    }
}

impl fmt::Debug for ShardId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ShardId({:04b} ", self.depth())?;
        let p = self.id_to_path();
        for quad in p {
            write!(f, "{:02b} ", quad as u8)?;
        }
        write!(f, "[{}])", self.0)
    }
}

impl From <u32> for ShardId {
    fn from(value: u32) -> Self {
        Self(value)
    }
}

impl From <ShardId> for u32 {
    fn from(value: ShardId) -> Self {
        value.0
    }
}