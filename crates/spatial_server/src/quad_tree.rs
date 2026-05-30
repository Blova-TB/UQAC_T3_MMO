use crate::shard_id::{Quadrant, ShardId};
use ahash::{AHashMap, AHashSet};
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
    pub(crate) fn center(&self) -> Vec2<f32> {
        (self.min + self.max) * 0.5
    }
}

pub struct QuadTree {
    pub bounds: Rect,
    pub depth: u8,
    pub max_depth: u8,
    pub shard_id: Option<ShardId>,
    pub children: Option<Box<[QuadTree; 4]>>,
    pub players: AHashMap<u32, Vec2<f32>>, // client_id -> position
    pub server_occupation: Option<f32>,
    pub last_subdivide_time: Option<std::time::Instant>,
}

impl QuadTree {
    pub fn new(bounds: Rect, depth: u8, max_depth: u8, shard_id: ShardId) -> Self {
        Self {
            bounds,
            depth,
            max_depth,
            children: None,
            shard_id: Some(shard_id),
            players: AHashMap::new(),
            server_occupation: None,
            last_subdivide_time: None,
        }
    }

    pub fn shard_id_for(&self, pos: Vec2<f32>) -> Option<ShardId> {
        Some(self.shard_for(pos)?.shard_id?)
    }

    pub fn shard_for(&self, pos: Vec2<f32>) -> Option<&QuadTree> {
        if !self.bounds.contains(pos) {
            return None;
        }

        if let Some(children) = &self.children {
            for child in children.iter() {
                if child.bounds.contains(pos) {
                    return child.shard_for(pos);
                }
            }
        }

        Some(self)
    }

    pub fn shards_near(&self, pos: Vec2<f32>, margin: f32) -> Vec<ShardId> {
        let mut distinct_shards = AHashSet::new();

        let query_rect = Rect {
            min: pos - Vec2::splat(margin),
            max: pos + Vec2::splat(margin),
        };

        self.collect_intersecting_shards(query_rect, &mut distinct_shards);

        distinct_shards.into_iter().collect()
    }

    fn collect_intersecting_shards(&self, query_rect: Rect, result: &mut AHashSet<ShardId>) {
        if !self.bounds.intersects(&query_rect) {
            return;
        }

        if let Some(children) = &self.children {
            for child in children.iter() {
                child.collect_intersecting_shards(query_rect, result);
            }
        } else if let Some(id) = self.shard_id {
            result.insert(id);
        }
    }

    pub fn insert_player(&mut self, client_id: u32, pos: Vec2<f32>) -> Option<ShardId> {
        if !self.bounds.contains(pos) {
            return None;
        }

        if let Some(children) = &mut self.children {
            for child in children.iter_mut() {
                if let Some(shard) = child.insert_player(client_id, pos) {
                    return Some(shard);
                }
            }
            return None;
        }

        self.players.insert(client_id, pos);
        self.shard_id
    }
    
    pub fn remove_player(&mut self, client_id: u32, shard_id: ShardId) -> Option<()> {

        let mut current_node = self;

        for quadrant in shard_id.id_to_path() {
            current_node = current_node.get_shard(quadrant)?;
        };

        current_node.players.remove(&client_id)?;
        Some(())
    }

    pub fn subdivide_quad_tree(&mut self) -> Vec<(u32, ShardId)> {
        let center = self.bounds.center();
        let min = self.bounds.min;
        let max = self.bounds.max;

        let create_child = |min_b: Vec2<f32>, max_b: Vec2<f32>, quadrant: Quadrant| -> QuadTree {
            QuadTree::new(
                Rect {
                    min: min_b,
                    max: max_b,
                },
                self.depth + 1,
                self.max_depth,
                self.shard_id.unwrap().new_id_for_child(quadrant),
            )
        };

        let tl = create_child(
            Vec2::new(min.x, center.y),
            Vec2::new(center.x, max.y),
            Quadrant::TopLeft,
        );
        let tr = create_child(center, max, Quadrant::TopRight);
        let bl = create_child(min, center, Quadrant::BottomLeft);
        let br = create_child(
            Vec2::new(center.x, min.y),
            Vec2::new(max.x, center.y),
            Quadrant::BottomRight,
        );

        let mut children = Box::new([tl, tr, bl, br]);

        let mut player_moved: Vec<(u32, ShardId)> = Vec::new();

        for (id, pos) in self.players.drain() {
            for child in children.iter_mut() {
                if child.bounds.contains(pos) {
                    child.players.insert(id, pos);
                    if let Some(tiprout) = child.shard_id {
                        player_moved.push((id, tiprout));
                    }
                    break;
                }
            }
        }

        self.players.clear();
        self.children = Some(children);
        self.shard_id = None;

        player_moved
    }

    pub fn get_shard(&mut self, quad: Quadrant) -> Option<&mut QuadTree> {
        let children = self.children.as_mut()?;
        Some(&mut children[quad as usize])
    }
    
    pub fn get_shard_by_id(&mut self, shard_id: ShardId) -> Option<&mut QuadTree> {
        let mut current_node = self;

        for quadrant in shard_id.id_to_path() {
            current_node = current_node.get_shard(quadrant)?;
        }

        Some(current_node)
    }
}
