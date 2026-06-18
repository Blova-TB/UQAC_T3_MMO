use slotmap::new_key_type;
use crate::shard_id::ShardId;
use mathtools::Vec2;

// ============================================================================
// 1. GÉOMÉTRIE ET MATHÉMATIQUES
// ============================================================================

/// Représente un point ou un vecteur dans l'espace 2D.
#[derive(Debug, Clone, Copy, Default)]
pub struct Point2D {
    pub x: f32,
    pub y: f32,
}

impl Point2D {
    /// Calcule la distance au carré entre deux points.
    /// OPTIMISATION : Évite la racine carrée coûteuse (sqrt) très gourmande en CPU.
    /// Parfait pour comparer des distances (si A² > B², alors A > B).
    #[inline]
    pub fn distance_sq(&self, other: &Point2D) -> f32 {
        let dx = self.x - other.x;
        let dy = self.y - other.y;
        dx * dx + dy * dy
    }

    /// Méthode de commodité pour convertir rapidement en Vec2 en ligne.
    #[inline]
    pub fn as_vec2(&self) -> Vec2<f32> {
        Vec2 { x: self.x, y: self.y }
    }
}

// ============================================================================
// CONVERSIONS IDIOMATIQUES RUST (Zero-Cost Abstractions)
// ============================================================================

/// Permet la conversion de mathtools::Vec2 vers Point2D
impl From<Vec2<f32>> for Point2D {
    #[inline]
    fn from(vec: Vec2<f32>) -> Self {
        Self { x: vec.x, y: vec.y }
    }
}

/// Permet la conversion de Point2D vers mathtools::Vec2
impl From<Point2D> for Vec2<f32> {
    #[inline]
    fn from(point: Point2D) -> Self {
        Self { x: point.x, y: point.y }
    }
}

/// AABB (Axis-Aligned Bounding Box) : Une boîte rectangulaire alignée sur les axes X et Y.
/// Utilisée pour le clipping spatial et la définition des "Ghost Zones" (Interest Management).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct AABB {
    pub min_x: f32, pub min_y: f32,
    pub max_x: f32, pub max_y: f32,
}

impl AABB {
    /// Vérifie si un point se trouve à l'intérieur strict de la boîte englobante.
    pub fn contains(&self, p: &Point2D) -> bool {
        p.x >= self.min_x && p.x <= self.max_x && p.y >= self.min_y && p.y <= self.max_y
    }
}

// ============================================================================
// 2. IDENTIFIANTS UNIQUES (ECS & SLOTMAP)
// ============================================================================

// Génération de clés fortement typées pour éviter de mélanger un ID de joueur avec un ID de Shard.
new_key_type! {
    pub struct PlayerKey;
}

// ============================================================================
// 3. ENTITÉS DU MONDE
// ============================================================================

/// Représente un joueur (ou une entité réseau) dans le monde.

pub struct Player {
    pub pos: Point2D,
    pub current_shard: ShardId, // <-- ici
    pub ghost_shards: Vec<ShardId>, // <-- ici
}

/// Représente le "germe" d'une zone de Voronoï (un serveur physique/logique).
pub struct Shard {
    pub pos: Point2D,

    /// L'âge de la shard (en ticks). Utilisé pour empêcher les divisions/fusions en chaîne (Cooldown).
    pub spawn_tick: u64,
}