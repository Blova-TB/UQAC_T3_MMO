use mathtools::Vec2;
// On retire complètement slotmap car on n'en a plus besoin ici !
use spade::{DelaunayTriangulation, HasPosition, Point2, Triangulation};
use rustc_hash::FxHashMap;
use serde::Serialize;
use crate::shard_id::{ShardId, ShardIdGenerator};
use crate::client_id::ClientId; // <-- Ajout du ClientId réseau
use crate::shared::{Point2D, Player, Shard, AABB}; // PlayerKey retiré

#[derive(Serialize)]
pub struct VizPoint { pub x: f32, pub y: f32 }

#[derive(Serialize)]
pub struct VizState {
    pub map_width: f32,
    pub map_height: f32,
    pub shards: Vec<VizShard>,
    pub polygons: Vec<VizPolygon>,
    pub ghost_aabbs: Vec<VizAABB>,
    pub players: Vec<VizPlayer>,
}

#[derive(Serialize)]
pub struct VizShard { pub id: String, pub x: f32, pub y: f32 }

#[derive(Serialize)]
pub struct VizPolygon { pub shard_id: String, pub vertices: Vec<VizPoint> }

#[derive(Serialize)]
pub struct VizAABB { pub shard_id: String, min_x: f32, min_y: f32, max_x: f32, max_y: f32 }

#[derive(Serialize)]
pub struct VizPlayer { pub id: String, pub x: f32, pub y: f32, pub shard_id: String }

// ============================================================================
// 1. STRUCTURES ET TYPES DE DONNÉES
// ============================================================================

const HYSTERESIS: f32 = 15.0;

#[derive(Clone, PartialEq, Debug)]
pub enum VertexKind { Real(ShardId), Ghost }

#[derive(Clone, PartialEq, Debug)]
pub struct ShardVertex { pub kind: VertexKind, pub point: Point2<f64> }

impl HasPosition for ShardVertex {
    type Scalar = f64;
    fn position(&self) -> Point2<f64> { self.point }
}

pub struct ShardCellData {
    pub neighbors: Vec<ShardId>,
    pub safe_radius_sq: f32,
    pub ghost_aabb: AABB,
}

pub struct VoronoiConfig {
    pub split_occupancy_threshold: u8,
    pub merge_occupancy_threshold: u8,
    pub min_age_ticks: u64,
    pub max_merge_dist_sq: f32,
    pub ghost_margin: f32,
    pub map_width: f32,
    pub map_height: f32,
}

impl VoronoiConfig {
    pub fn new(
        split_occupancy_threshold: u8,
        merge_occupancy_threshold: u8,
        min_age_ticks: u64,
        max_merge_dist_sq: f32,
        ghost_margin: f32,
        map_width: f32,
        map_height: f32,
    ) -> Self {
        Self {
            split_occupancy_threshold,
            merge_occupancy_threshold,
            min_age_ticks,
            max_merge_dist_sq,
            ghost_margin,
            map_width,
            map_height,
        }
    }
}

impl Default for VoronoiConfig {
    fn default() -> Self {
        Self {
            split_occupancy_threshold: 80,
            merge_occupancy_threshold: 40,
            min_age_ticks: 15,
            max_merge_dist_sq: 400.0 * 400.0,
            ghost_margin: 150.0,
            map_width: 100000.0,
            map_height: 100000.0,
        }
    }
}

#[derive(Clone, Copy)]
struct ShardMetrics {
    count: u32,
    min_bound: Point2D,
    max_bound: Point2D,
}

// ============================================================================
// 2. LE SERVICE VORONOI
// ============================================================================

pub struct Voronoi {
    // Changement ici : On utilise FxHashMap avec ClientId
    players: FxHashMap<ClientId, Player>,
    shards: FxHashMap<ShardId, Shard>,
    cells: FxHashMap<ShardId, ShardCellData>,
    id_generator: ShardIdGenerator,
    triangulation: DelaunayTriangulation<ShardVertex>,
    current_tick: u64,
    voronoi_interval: f32,
    voronoi_timer: f32,
    pub config: VoronoiConfig,
    pub shard_occupancies: FxHashMap<ShardId, u8>,
}

impl Default for Voronoi {
    fn default() -> Self { Self::new(5.0, VoronoiConfig::default()) }
}

impl Voronoi {
    pub fn new(updates_per_second: f32, config: VoronoiConfig) -> Self {
        let mut spatial = Self {
            players: FxHashMap::default(),
            shards: FxHashMap::default(),
            cells: FxHashMap::default(),
            id_generator: ShardIdGenerator::new(),
            triangulation: DelaunayTriangulation::new(),
            current_tick: 0,
            voronoi_interval: 1.0 / updates_per_second,
            voronoi_timer: 0.0,
            config,
            shard_occupancies: FxHashMap::default(),
        };
        spatial.rebuild_triangulation();
        spatial
    }

    pub fn update_map_size(&mut self, width: f32, height: f32) {
        if self.config.map_width != width || self.config.map_height != height {
            self.config.map_width = width;
            self.config.map_height = height;
            self.update_voronoi_cache();
        }
    }

    pub fn init_base_shards(&mut self, x: f32, y: f32) -> ShardId {
        let p1 = Point2D { x, y };
        let root_id = ShardId::ROOT;

        self.shards.insert(root_id, Shard { pos: p1, spawn_tick: self.current_tick });
        self.rebuild_triangulation();
        root_id
    }

    // Changement de signature : PlayerKey -> ClientId
    pub fn add_player(&mut self, key: ClientId, pos: Point2D, initial_shard: ShardId) {
        self.players.insert(key, Player { pos, current_shard: initial_shard, ghost_shards: Vec::new() });
    }

    pub fn insert_player(&mut self, client_id: ClientId, pos: Point2D, shard: ShardId) {
        self.players.insert(client_id, Player { pos, current_shard: shard, ghost_shards: Vec::new() });
    }

    // Changement de signature et ajout de `&` pour le get_mut
    pub fn update_player_position(&mut self, key: ClientId, new_pos: Point2D) {
        if let Some(player) = self.players.get_mut(&key) { player.pos = new_pos; }
    }

    pub fn update_shard_occupancies(&mut self, occupancies: FxHashMap<ShardId, u8>) {
        self.shard_occupancies = occupancies;
    }

    // Changement de signature
    pub fn update_player_shard(&mut self, key: ClientId, new_shard: ShardId) {
        if let Some(player) = self.players.get_mut(&key) { player.current_shard = new_shard; }
    }

    pub fn tick(&mut self, dt: f32) -> bool {
        self.voronoi_timer += dt;

        if self.voronoi_timer >= self.voronoi_interval {
            self.voronoi_timer -= self.voronoi_interval;
            self.current_tick += 1;

            let geometry_changed = self.update_dynamics();
            self.relax_shards(0.1);
            self.rebuild_triangulation();

            return true;
        }
        false
    }

    pub fn rebuild_triangulation(&mut self) {
        self.triangulation = DelaunayTriangulation::new();

        let max_dim = self.config.map_width.max(self.config.map_height);
        let margin = max_dim * 100.0;

        let center_x = self.config.map_width / 2.0;
        let center_y = self.config.map_height / 2.0;

        let ghosts = [
            Point2::new((center_x - margin) as f64, (center_y - margin) as f64),
            Point2::new((center_x + margin) as f64, (center_y - margin) as f64),
            Point2::new((center_x + margin) as f64, (center_y + margin) as f64),
            Point2::new((center_x - margin) as f64, (center_y + margin) as f64)
        ];

        for point in ghosts {
            self.triangulation.insert(ShardVertex { kind: VertexKind::Ghost, point }).unwrap();
        }

        for (&key, shard) in self.shards.iter() {
            self.triangulation.insert(ShardVertex {
                kind: VertexKind::Real(key),
                point: Point2::new(shard.pos.x as f64, shard.pos.y as f64)
            }).unwrap();
        }

        self.update_voronoi_cache();
    }

    pub fn relax_shards(&mut self, lerp_factor: f32) {
        let mut metrics: FxHashMap<ShardId, (f32, f32, u32)> = FxHashMap::default();

        for (_, player) in self.players.iter() {
            let entry = metrics.entry(player.current_shard).or_insert((0.0, 0.0, 0));
            entry.0 += player.pos.x;
            entry.1 += player.pos.y;
            entry.2 += 1;
        }

        let mut geometry_changed = false;

        for (key, shard) in self.shards.iter_mut() {
            if let Some((sum_x, sum_y, count)) = metrics.get(key) {
                if *count > 0 {
                    let cx = sum_x / (*count as f32);
                    let cy = sum_y / (*count as f32);

                    let dx = cx - shard.pos.x;
                    let dy = cy - shard.pos.y;

                    if dx * dx + dy * dy > 1.0 {
                        shard.pos.x += dx * lerp_factor;
                        shard.pos.y += dy * lerp_factor;
                        geometry_changed = true;
                    }
                }
            }
        }

        if geometry_changed {
            self.rebuild_triangulation();
        }
    }

    fn update_dynamics(&mut self) -> bool {
        let mut metrics: FxHashMap<ShardId, ShardMetrics> = FxHashMap::default();

        for (_, player) in self.players.iter() {
            let m = metrics.entry(player.current_shard).or_insert(ShardMetrics {
                count: 0,
                min_bound: Point2D { x: f32::MAX, y: f32::MAX },
                max_bound: Point2D { x: f32::MIN, y: f32::MIN },
            });
            m.count += 1;
            m.min_bound.x = m.min_bound.x.min(player.pos.x);
            m.min_bound.y = m.min_bound.y.min(player.pos.y);
            m.max_bound.x = m.max_bound.x.max(player.pos.x);
            m.max_bound.y = m.max_bound.y.max(player.pos.y);
        }

        let mut to_remove = Vec::new();
        let mut new_spawns = Vec::new();

        for (&key, shard) in self.shards.iter() {
            let current_occupancy = self.shard_occupancies.get(&key).copied().unwrap_or(0);

            if let Some(m) = metrics.get(&key) {
                if current_occupancy >= self.config.split_occupancy_threshold && self.current_tick >= shard.spawn_tick + self.config.min_age_ticks {
                    to_remove.push(key);

                    let spread_x = m.max_bound.x - m.min_bound.x;
                    let spread_y = m.max_bound.y - m.min_bound.y;

                    let (p1, p2) = if spread_x > spread_y {
                        (Point2D { x: m.min_bound.x, y: shard.pos.y }, Point2D { x: m.max_bound.x, y: shard.pos.y })
                    } else {
                        (Point2D { x: shard.pos.x, y: m.min_bound.y }, Point2D { x: shard.pos.x, y: m.max_bound.y })
                    };
                    new_spawns.push(p1);
                    new_spawns.push(p2);
                }
            }
        }

        if to_remove.is_empty() {
            let shards_vec: Vec<_> = self.shards.iter().collect();
            let mut best_pair = None;
            let mut min_dist_sq = f32::MAX;

            for i in 0..shards_vec.len() {
                for j in (i+1)..shards_vec.len() {
                    let (&k1, s1) = shards_vec[i];
                    let (&k2, s2) = shards_vec[j];

                    if self.current_tick < s1.spawn_tick + self.config.min_age_ticks || self.current_tick < s2.spawn_tick + self.config.min_age_ticks {
                        continue;
                    }

                    let occ1 = self.shard_occupancies.get(&k1).copied().unwrap_or(0);
                    let occ2 = self.shard_occupancies.get(&k2).copied().unwrap_or(0);
                    let combined_occupancy = occ1.saturating_add(occ2);

                    if combined_occupancy <= self.config.merge_occupancy_threshold {
                        let dist_sq = s1.pos.distance_sq(&s2.pos);
                        if dist_sq < min_dist_sq && dist_sq < self.config.max_merge_dist_sq {
                            min_dist_sq = dist_sq;
                            best_pair = Some((k1, k2, s1.pos, s2.pos));
                        }
                    }
                }
            }

            if let Some((k1, k2, p1, p2)) = best_pair {
                to_remove.push(k1);
                to_remove.push(k2);
                new_spawns.push(Point2D { x: (p1.x + p2.x)/2.0, y: (p1.y + p2.y)/2.0 });
            }
        }

        let geometry_changed = !to_remove.is_empty();

        for key in to_remove {
            self.shards.remove(&key);
            self.id_generator.free(key);
        }

        for pos in new_spawns {
            let new_key = self.id_generator.generate();
            self.shards.insert(new_key, Shard { pos, spawn_tick: self.current_tick });
        }

        geometry_changed
    }

    pub fn update_voronoi_cache(&mut self) {
        self.cells.clear();
        let map_min = Point2D { x: 0.0, y: 0.0 };
        let map_max = Point2D { x: self.config.map_width, y: self.config.map_height };

        for vertex in self.triangulation.vertices() {
            let VertexKind::Real(key) = vertex.data().kind else { continue; };

            let mut neighbors = Vec::new();
            let mut min_neighbor_dist_sq = f32::MAX;
            let mut raw_polygon = Vec::with_capacity(8);

            let face = vertex.as_voronoi_face();
            for edge in face.adjacent_edges() {
                if let spade::handles::VoronoiVertex::Inner(inner) = edge.from() {
                    let p = inner.circumcenter();
                    raw_polygon.push(Point2D { x: p.x as f32, y: p.y as f32 });
                }
            }

            for edge in vertex.out_edges() {
                if let VertexKind::Real(n_key) = edge.to().data().kind {
                    neighbors.push(n_key);
                    let dist_sq = edge.to().position().distance_2(vertex.position()) as f32;
                    if dist_sq < min_neighbor_dist_sq { min_neighbor_dist_sq = dist_sq; }
                }
            }

            let clipped = clip_polygon_aabb(&raw_polygon, &map_min, &map_max);
            let mut min_x = f32::MAX; let mut min_y = f32::MAX;
            let mut max_x = f32::MIN; let mut max_y = f32::MIN;

            for p in &clipped {
                min_x = min_x.min(p.x); min_y = min_y.min(p.y);
                max_x = max_x.max(p.x); max_y = max_y.max(p.y);
            }

            let margin = self.config.ghost_margin;
            let ghost_aabb = AABB {
                min_x: (min_x - margin).max(0.0),
                min_y: (min_y - margin).max(0.0),
                max_x: (max_x + margin).min(map_max.x),
                max_y: (max_y + margin).min(map_max.y),
            };

            let min_dist = min_neighbor_dist_sq.sqrt();
            let safe_radius_sq = ((min_dist / 2.0) - HYSTERESIS).max(0.0).powi(2);

            self.cells.insert(key, ShardCellData { neighbors, safe_radius_sq, ghost_aabb });
        }
    }

    // Retourne maintenant un FxHashMap avec des ClientId
    pub fn compute_ghost_visibility(&self) -> FxHashMap<ClientId, Vec<ShardId>> {
        let mut visibilities = FxHashMap::default();

        for (&client_id, player) in self.players.iter() {
            let mut visible_shards = Vec::new();
            for (&shard_key, cell) in self.cells.iter() {
                if shard_key == player.current_shard { continue; }
                if cell.ghost_aabb.contains(&player.pos) {
                    visible_shards.push(shard_key);
                }
            }
            if !visible_shards.is_empty() {
                visibilities.insert(client_id, visible_shards);
            }
        }
        visibilities
    }

    pub fn find_nearest_shard(&self, pos: Point2D) -> ShardId {
        let mut min_dist = f32::MAX;
        let mut nearest = *self.shards.keys().next().expect("Aucune shard présente !");
        for (&key, shard) in self.shards.iter() {
            let dist = pos.distance_sq(&shard.pos);
            if dist < min_dist { min_dist = dist; nearest = key; }
        }
        nearest
    }

    pub(crate) fn shard_id_for(&self, p0: Point2D) -> Option<ShardId> {
        if self.shards.is_empty() { None } else { Some(self.find_nearest_shard(p0)) }
    }

    fn evaluate_handoff(&self, pos: Point2D, current_shard: ShardId) -> ShardId {
        if !self.shards.contains_key(&current_shard) {
            return self.find_nearest_shard(pos);
        }
        if let Some(cell_data) = self.cells.get(&current_shard) {
            if let Some(current) = self.shards.get(&current_shard) {
                let current_dist_sq = pos.distance_sq(&current.pos);

                if current_dist_sq < cell_data.safe_radius_sq {
                    return current_shard;
                }

                let mut best_dist_sq = current_dist_sq;
                let mut best_key = current_shard;

                for &neighbor_key in &cell_data.neighbors {
                    if let Some(n_shard) = self.shards.get(&neighbor_key) {
                        let dist_sq = pos.distance_sq(&n_shard.pos);
                        if dist_sq < best_dist_sq {
                            best_dist_sq = dist_sq;
                            best_key = neighbor_key;
                        }
                    }
                }

                if best_key != current_shard && best_dist_sq < (current_dist_sq - (HYSTERESIS * HYSTERESIS)) {
                    return best_key;
                }
            }
        }
        current_shard
    }

    // Retourne un tuple comprenant le ClientId
    pub fn compute_pending_handoffs(&self) -> Vec<(ClientId, ShardId, ShardId)> {
        let mut handoffs = Vec::new();
        for (&client_id, player) in self.players.iter() {
            let optimal_shard = self.evaluate_handoff(player.pos, player.current_shard);
            if optimal_shard != player.current_shard {
                handoffs.push((client_id, player.current_shard, optimal_shard));
            }
        }
        handoffs
    }

    pub fn get_shards(&self) -> impl Iterator<Item = (&ShardId, &Shard)> { self.shards.iter() }

    // Modification de l'itérateur pour retourner ClientId
    pub fn get_players(&self) -> impl Iterator<Item = (&ClientId, &Player)> { self.players.iter() }

    pub fn get_cells(&self) -> impl Iterator<Item = (&ShardId, &ShardCellData)> { self.cells.iter() }

    pub fn get_voronoi_polygons(&self) -> Vec<(ShardId, Vec<Point2D>)> {
        let mut results = Vec::new();
        let min_b = Point2D { x: 0.0, y: 0.0 };
        let max_b = Point2D { x: self.config.map_width, y: self.config.map_height };
        for vertex in self.triangulation.vertices() {
            let VertexKind::Real(key) = vertex.data().kind else { continue; };
            let face = vertex.as_voronoi_face();
            let mut raw_polygon = Vec::with_capacity(8);
            for edge in face.adjacent_edges() {
                if let spade::handles::VoronoiVertex::Inner(inner) = edge.from() {
                    let p = inner.circumcenter();
                    raw_polygon.push(Point2D { x: p.x as f32, y: p.y as f32 });
                }
            }
            let clipped = clip_polygon_aabb(&raw_polygon, &min_b, &max_b);
            if !clipped.is_empty() { results.push((key, clipped)); }
        }
        results
    }

    pub fn extract_viz_state(&self) -> String {
        let mut state = VizState {
            map_width: self.config.map_width,
            map_height: self.config.map_height,
            shards: Vec::new(),
            polygons: Vec::new(),
            ghost_aabbs: Vec::new(),
            players: Vec::new(),
        };

        for (&client_id, player) in self.players.iter() {
            state.players.push(VizPlayer {
                id: format!("{:?}", client_id), // On s'assure d'imprimer l'ID du client proprement
                x: player.pos.x,
                y: player.pos.y,
                shard_id: format!("{:?}", player.current_shard),
            });
        }

        for (&s_key, shard) in self.shards.iter() {
            state.shards.push(VizShard {
                id: format!("{:?}", s_key),
                x: shard.pos.x,
                y: shard.pos.y,
            });

            if let Some(cell) = self.cells.get(&s_key) {
                state.ghost_aabbs.push(VizAABB {
                    shard_id: format!("{:?}", s_key),
                    min_x: cell.ghost_aabb.min_x,
                    min_y: cell.ghost_aabb.min_y,
                    max_x: cell.ghost_aabb.max_x,
                    max_y: cell.ghost_aabb.max_y,
                });
            }
        }

        for (s_key, poly) in self.get_voronoi_polygons() {
            let vertices = poly.into_iter().map(|p| VizPoint { x: p.x, y: p.y }).collect();
            state.polygons.push(VizPolygon {
                shard_id: format!("{:?}", s_key),
                vertices,
            });
        }

        serde_json::to_string(&state).unwrap_or_else(|_| "{}".to_string())
    }
}

pub fn clip_polygon_aabb(poly: &[Point2D], min_b: &Point2D, max_b: &Point2D) -> Vec<Point2D> {
    let mut input = poly.to_vec();
    let mut output = Vec::with_capacity(8);

    clip_edge(&mut input, &mut output, |p| p.x >= min_b.x, |p1, p2| {
        let t = (min_b.x - p1.x) / (p2.x - p1.x); Point2D { x: min_b.x, y: p1.y + t * (p2.y - p1.y) }
    }); std::mem::swap(&mut input, &mut output); output.clear();

    clip_edge(&mut input, &mut output, |p| p.x <= max_b.x, |p1, p2| {
        let t = (max_b.x - p1.x) / (p2.x - p1.x); Point2D { x: max_b.x, y: p1.y + t * (p2.y - p1.y) }
    }); std::mem::swap(&mut input, &mut output); output.clear();

    clip_edge(&mut input, &mut output, |p| p.y >= min_b.y, |p1, p2| {
        let t = (min_b.y - p1.y) / (p2.y - p1.y); Point2D { x: p1.x + t * (p2.x - p1.x), y: min_b.y }
    }); std::mem::swap(&mut input, &mut output); output.clear();

    clip_edge(&mut input, &mut output, |p| p.y <= max_b.y, |p1, p2| {
        let t = (max_b.y - p1.y) / (p2.y - p1.y); Point2D { x: p1.x + t * (p2.x - p1.x), y: max_b.y }
    });
    output
}

#[inline(always)]
fn clip_edge<F, I>(input: &mut Vec<Point2D>, output: &mut Vec<Point2D>, inside: F, intersect: I)
where F: Fn(&Point2D) -> bool, I: Fn(&Point2D, &Point2D) -> Point2D {
    if input.is_empty() { return; }
    let mut prev = *input.last().unwrap();
    let mut prev_in = inside(&prev);

    for curr in input.iter() {
        let curr_in = inside(curr);
        if curr_in != prev_in { output.push(intersect(&prev, curr)); }
        if curr_in { output.push(*curr); }
        prev = *curr; prev_in = curr_in;
    }
}