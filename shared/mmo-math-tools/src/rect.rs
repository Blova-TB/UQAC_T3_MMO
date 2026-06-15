use mathtools::Vec2;

#[derive(Clone, Copy, Debug)]
pub struct Rect {
    pub min: Vec2<f32>,
    pub max: Vec2<f32>,
}

impl Rect {
    #[inline]
    pub fn contains(&self, pos: Vec2<f32>) -> bool {
        pos.x >= self.min.x && pos.x <= self.max.x && pos.y >= self.min.y && pos.y <= self.max.y
    }

    #[inline]
    pub fn intersects(&self, other: &Rect) -> bool {
        !(self.max.x < other.min.x
            || self.min.x > other.max.x
            || self.max.y < other.min.y
            || self.min.y > other.max.y)
    }

    #[inline]
    pub fn center(&self) -> Vec2<f32> {
        (self.min + self.max) * 0.5
    }
}