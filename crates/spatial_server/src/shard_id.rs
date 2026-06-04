use std::fmt;
use mathtools::Vec2;
use crate::quad_tree::Rect;
use shared::custom_id::{CustomId, IdType};

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

/// ShardId est maintenant un wrapper de CustomId
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct ShardId(CustomId);

impl ShardId {
    // Plus besoin des masques de SIGN, CustomId isole déjà les 28 bits !
    const DEPTH_MASK: u32 = 0x0F00_0000;
    const QUADRANT_MASK: u32 = 0x00FF_FFFF;
    const DEPTH_SHIFT: u8 = 24;
    pub const MAX_DEPTH: u8 = 12; // 24 bits dispos / 2 bits par quad = 12

    // On initialise le ROOT proprement avec le constructeur de CustomId
    pub const ROOT: Self = Self(CustomId::new_unchecked(IdType::Server, 0));

    #[inline]
    pub fn depth(self) -> u8 {
        // self.0.value() retourne uniquement les 28 bits, ignorant le type
        ((self.0.value() & Self::DEPTH_MASK) >> Self::DEPTH_SHIFT) as u8
    }

    pub fn new_id_for_child(self, quadrant: Quadrant) -> Self {
        let current_depth = self.depth();
        assert!(
            current_depth < Self::MAX_DEPTH,
            "Profondeur maximale du QuadTree atteinte ({})", Self::MAX_DEPTH
        );

        let new_depth = current_depth + 1;

        // On travaille uniquement sur l'espace des 28 bits (la valeur)
        let mut new_val = self.0.value() & Self::QUADRANT_MASK;
        new_val |= (new_depth as u32) << Self::DEPTH_SHIFT;

        // Ajout du quadrant
        let shift = Self::DEPTH_SHIFT - (2 * new_depth);
        new_val |= (quadrant as u32) << shift;

        // On encapsule avec la certitude que c'est un IdType::Server
        Self(CustomId::new_unchecked(IdType::Server, new_val))
    }

    pub fn id_to_path(self) -> Vec<Quadrant> {
        let depth = self.depth();
        let mut result = Vec::with_capacity(depth as usize);
        let val = self.0.value();

        for i in 1..=depth {
            let shift = Self::DEPTH_SHIFT - (2 * i);
            let quad_bits = (val >> shift) & 0b11;

            let quad = match quad_bits {
                0b00 => Quadrant::TopLeft,
                0b01 => Quadrant::TopRight,
                0b10 => Quadrant::BottomLeft,  // <- Bug corrigé ici
                0b11 => Quadrant::BottomRight, // <- Bug corrigé ici
                _ => unreachable!(),
            };
            result.push(quad);
        }

        result
    }
    
    pub fn get_parent_shard_id(self) -> Option<Self> {
        let depth = self.depth();
        if depth == 0 {
            return None; // Le ROOT n'a pas de parent
        }

        let mut parent_val = self.0.value() & Self::QUADRANT_MASK;
        parent_val &= !(0b11 << (Self::DEPTH_SHIFT - (2 * depth)));
        parent_val &= !(Self::DEPTH_MASK);
        parent_val |= ((depth - 1) as u32) << Self::DEPTH_SHIFT;

        Some(Self(CustomId::new_unchecked(IdType::Server, parent_val)))
    }

    pub fn is_ancestor_of(self, other: Self) -> bool {
        let self_depth = self.depth();
        let other_depth = other.depth();

        if self_depth >= other_depth {
            return false; // Un shard ne peut pas être l'ancêtre d'un autre shard de même profondeur ou plus profond
        }

        // on compare les morceaux des id
        let shift = Self::DEPTH_SHIFT - (self_depth << 1);
        ((self.0.value() ^ other.0.value()) & Self::QUADRANT_MASK) >> shift == 0
    }

    pub fn is_descendant_of(self, other: Self) -> bool {
        other.is_ancestor_of(self)
    }
}

// L'affichage de debug reste identique, ce qui est très pratique
impl fmt::Debug for ShardId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ShardId({:04b} ", self.depth())?;
        let p = self.id_to_path();
        for quad in p {
            write!(f, "{:02b} ", quad as u8)?;
        }
        write!(f, "[{}])", self.0.as_u32()) // On affiche le u32 complet
    }
}

// ==========================================
//   CONVERSIONS SÉCURISÉES (TRY_FROM / FROM)
// ==========================================

/// Permet la conversion stricte d'un CustomId en ShardId
impl TryFrom<CustomId> for ShardId {
    type Error = &'static str;

    fn try_from(id: CustomId) -> Result<Self, Self::Error> {
        if id.id_type()? != IdType::Server {
            return Err("Le CustomId fourni n'est pas du type Serveur (Shard)");
        }
        Ok(Self(id))
    }
}

/// Transforme silencieusement un ShardId en CustomId
/// (Utile pour le passer à tes services comme SpatialService ou le réseau)
impl From<ShardId> for CustomId {
    fn from(shard_id: ShardId) -> Self {
        shard_id.0
    }
}

/// Au cas où un module doive absolument lire un u32 brut
impl From<ShardId> for u32 {
    fn from(value: ShardId) -> Self {
        value.0.as_u32()
    }
}