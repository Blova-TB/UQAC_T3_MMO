use slotmap::new_key_type;

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
    pub struct ShardKey;
}

// ============================================================================
// 3. ENTITÉS DU MONDE
// ============================================================================

/// Représente un joueur (ou une entité réseau) dans le monde.
pub struct Player {
    pub pos: Point2D,

    /// Le serveur (Shard) qui possède l'autorité absolue sur ce joueur.
    pub current_shard: ShardKey,

    /// Liste des serveurs voisins où le joueur est répliqué en tant que fantôme (Interest Management).
    pub ghost_shards: Vec<ShardKey>,
}

/// Représente le "germe" d'une zone de Voronoï (un serveur physique/logique).
pub struct Shard {
    pub pos: Point2D,

    /// L'âge de la shard (en ticks). Utilisé pour empêcher les divisions/fusions en chaîne (Cooldown).
    pub spawn_tick: u64,
}