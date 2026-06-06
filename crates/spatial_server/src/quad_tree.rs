use crate::shard_id::{Quadrant, ShardId};
use crate::client_id::ClientId;
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
    pub players: AHashMap<ClientId, Vec2<f32>>, // client_id -> position
    pub server_occupation: Option<u8>,
    pub creation_date: std::time::Instant,
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
            creation_date: std::time::Instant::now(),
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

    pub fn insert_player(&mut self, client_id: ClientId, pos: Vec2<f32>) -> Option<ShardId> {
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

    pub fn hard_update_player_position(&mut self, client_id: ClientId, shard_id: ShardId, new_pos: Vec2<f32>) -> Option<Vec2<f32>> {
        let current_node = self.get_shard_by_id_mut(shard_id)?;
        current_node.players.insert(client_id, new_pos)
    }

    pub fn remove_player(&mut self, client_id: ClientId, shard_id: ShardId) -> Option<Vec2<f32>> {
        let mut current_node = self;

        for quadrant in shard_id.id_to_path() {
            current_node = current_node.get_shard_mut(quadrant)?;
        };
        
        Some(current_node.players.remove(&client_id)?)
    }

    pub fn subdivide_quad_tree(&mut self) -> (Vec<(ClientId, ShardId, Vec2<f32>)>,Vec<(ClientId,Vec2<f32>)>) {
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

        let mut player_moved: Vec<(ClientId, ShardId, Vec2<f32>)> = Vec::new(); // <-- Utilisation de ClientId
        let mut player_loss: Vec<(ClientId, Vec2<f32>)> = Vec::new();
        for (id, pos) in self.players.drain() {
            if let Some(child) = children.iter_mut().find(|c| c.bounds.contains(pos)) {
                child.players.insert(id, pos);
                if let Some(new_shard_id) = child.shard_id {
                    player_moved.push((id, new_shard_id, pos));
                } else {
                    player_loss.push((id, pos));
                }
            } else {
                player_loss.push((id, pos));
            }
        }

        self.players.clear();
        self.children = Some(children);
        self.shard_id = None;
        self.creation_date = std::time::Instant::now();

        (player_moved,player_loss)
    }

    pub fn merge_quad_tree(&mut self, shard_id: ShardId) -> (Vec<(ClientId, ShardId)>, AHashSet<ShardId>) {

        let mut player_moved= (Vec::new(), AHashSet::new());

        self.get_all_player().into_iter().for_each(|(id, old_shard_id, pos)| {
            self.players.insert(id, pos);
            player_moved.0.push((id,old_shard_id));
            player_moved.1.insert(old_shard_id);
        });

        self.shard_id = Some(shard_id);
        self.children = None;
        self.creation_date = std::time::Instant::now();

        player_moved
    }

    pub fn get_shard_mut(&mut self, quad: Quadrant) -> Option<&mut QuadTree> {
        let children = self.children.as_mut()?;
        Some(&mut children[quad as usize])
    }
    
    pub fn get_shard(&self, quad: Quadrant) -> Option<&QuadTree> {
        let children = self.children.as_ref()?;
        Some(&children[quad as usize])
    }

    pub fn get_shard_by_id_mut(&mut self, shard_id: ShardId) -> Option<&mut QuadTree> {
        let mut current_node = self;

        for quadrant in shard_id.id_to_path() {
            current_node = current_node.get_shard_mut(quadrant)?;
        }

        Some(current_node)
    }
    
    pub fn get_shard_by_id(&self, shard_id: ShardId) -> Option<&QuadTree> {
        let mut current_node = self;

        for quadrant in shard_id.id_to_path() {
            current_node = current_node.get_shard(quadrant)?;
        }

        Some(current_node)
    }

    pub fn get_all_player(&self) -> Vec<(ClientId, ShardId, Vec2<f32>)> {
        if let Some(children) = &self.children {
            let mut players = Vec::new();
            for child in children.iter() {
                players.extend(child.get_all_player());
            }
            players
        } else if let Some(shard_id) = self.shard_id {
            self.players
                .iter()
                .map(|(id, pos)| (*id, shard_id, *pos))
                .collect()
        } else {
            Vec::new()
        }
    }

    pub fn get_occupation_somme(&self, min_elapsed_time: f32) -> u32 {
        if self.creation_date.elapsed() < std::time::Duration::from_secs_f32(min_elapsed_time) {
            return 100;
        }
        if let Some(children) = &self.children {
            children.iter().map(|child| child.get_occupation_somme(min_elapsed_time)).sum()
        } else {
            self.server_occupation.unwrap_or(100) as u32
        }
    }
}
