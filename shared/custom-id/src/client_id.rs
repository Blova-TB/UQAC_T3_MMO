use std::fmt;

use crate::custom_id::{CustomId, IdType};

/// ClientId encapsule un CustomId garanti d'être de type IdType::Client
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct ClientId(CustomId);

impl ClientId {
    /// Création sécurisée d'un nouveau ClientId
    #[inline]
    pub fn new(value: u32) -> Result<Self, &'static str> {
        let custom_id = CustomId::new(IdType::Client, value)?;
        Ok(Self(custom_id))
    }

    /// Création sans vérification (idéal pour des constantes ou des boucles rapides)
    #[inline]
    pub const fn new_unchecked(value: u32) -> Self {
        Self(CustomId::new_unchecked(IdType::Client, value))
    }

    /// Récupère le CustomId sous-jacent
    #[inline]
    pub fn as_custom_id(&self) -> CustomId {
        self.0
    }
}

// ==========================================
//   CONVERSIONS SÉCURISÉES (TRY_FROM / FROM)
// ==========================================

impl TryFrom<CustomId> for ClientId {
    type Error = &'static str;

    fn try_from(id: CustomId) -> Result<Self, Self::Error> {
        if id.id_type()? != IdType::Client {
            return Err("Le CustomId fourni n'est pas du type Client");
        }
        Ok(Self(id))
    }
}

impl From<ClientId> for CustomId {
    #[inline]
    fn from(client_id: ClientId) -> Self {
        client_id.0
    }
}

impl From<ClientId> for u32 {
    #[inline]
    fn from(value: ClientId) -> Self {
        value.0.as_u32()
    }
}

impl fmt::Debug for ClientId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ClientId({})", self.0.value())
    }
}