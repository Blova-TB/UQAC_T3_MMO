use std::fmt;
use custom_id::custom_id::{CustomId, IdType};
use rustc_hash::FxHashSet;
use rand::Rng;

// ============================================================================
// 1. DÉFINITION DU SHARD ID
// ============================================================================

/// ShardId est un wrapper typé et sécurisé autour de CustomId.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct ShardId(CustomId);

impl ShardId {
    /// L'ID 0 est strictement réservé pour le Root Shard (ou le point d'entrée initial).
    pub const ROOT: Self = Self(CustomId::new_unchecked(IdType::Server, 0));
}

impl fmt::Debug for ShardId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Affichage épuré, on ne garde que la valeur interne
        write!(f, "ShardId({})", self.0.value())
    }
}

// ============================================================================
// 2. GÉNÉRATEUR D'IDENTIFIANTS UNIQUES
// ============================================================================

/// Générateur garantissant des IDs aléatoires et non utilisés pour l'espace de 28 bits.
pub struct ShardIdGenerator {
    used_ids: FxHashSet<u32>,
}

impl Default for ShardIdGenerator {
    fn default() -> Self {
        Self::new()
    }
}

impl ShardIdGenerator {
    pub fn new() -> Self {
        let mut generator = Self {
            used_ids: FxHashSet::default(),
        };
        // On réserve immédiatement l'ID 0 pour ROOT afin qu'il ne soit jamais généré
        generator.used_ids.insert(0);
        generator
    }

    /// Génère un nouveau ShardId aléatoire garanti unique.
    pub fn generate(&mut self) -> ShardId {
        let mut rng = rand::thread_rng();

        loop {
            // On se limite à l'espace de 28 bits (0x0FFFFFFF) alloué par CustomId
            let val = rng.gen_range(1..=0x0FFF_FFFF);

            // `insert` renvoie true si la valeur n'était pas présente
            if self.used_ids.insert(val) {
                return ShardId(CustomId::new_unchecked(IdType::Server, val));
            }
        }
    }

    /// Libère un ShardId (utile lors d'un Merge où un Shard est détruit).
    pub fn free(&mut self, id: ShardId) {
        let val = id.0.value();
        if val != 0 { // On protège le ROOT
            self.used_ids.remove(&val);
        }
    }
}

// ============================================================================
// 3. CONVERSIONS SÉCURISÉES
// ============================================================================

impl TryFrom<CustomId> for ShardId {
    type Error = &'static str;

    fn try_from(id: CustomId) -> Result<Self, Self::Error> {
        if id.id_type()? != IdType::Server {
            return Err("Le CustomId fourni n'est pas du type Serveur (Shard)");
        }
        Ok(Self(id))
    }
}

impl From<ShardId> for CustomId {
    fn from(shard_id: ShardId) -> Self {
        shard_id.0
    }
}

impl From<ShardId> for u32 {
    fn from(value: ShardId) -> Self {
        value.0.as_u32()
    }
}