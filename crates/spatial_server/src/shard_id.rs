use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Quadrant {
    TopLeft = 0b00,
    TopRight = 0b01,
    BottomLeft = 0b10,
    BottomRight = 0b11,
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