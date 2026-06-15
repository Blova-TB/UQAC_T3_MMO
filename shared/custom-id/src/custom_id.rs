use serde::{Deserialize, Serialize};
use bitcode::{Decode, Encode};
use std::fmt;

#[derive(Serialize, Deserialize, Encode, Decode, Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum IdType {
    Client = 0,
    Server = 1,
    Entity = 2,
    Ghost = 3,
}

impl TryFrom<u8> for IdType {
    type Error = &'static str;

    #[inline]
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Client),
            1 => Ok(Self::Server),
            2 => Ok(Self::Entity),
            3 => Ok(Self::Ghost),
            _ => Err("Type d'ID inconnu"),
        }
    }
}

#[derive(Serialize, Deserialize, Encode, Decode, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CustomId(pub u32);

impl CustomId {
    pub const ID_MASK: u32 = 0x0FFF_FFFF;
    pub const TYPE_SHIFT: u8 = 28;

    #[inline]
    pub fn new(id_type: IdType, value: u32) -> Result<Self, &'static str> {
        if value > Self::ID_MASK {
            return Err("La valeur de l'ID dépasse la limite de 28 bits");
        }
        Ok(Self(((id_type as u32) << Self::TYPE_SHIFT) | value))
    }

    #[inline]
    pub const fn new_unchecked(id_type: IdType, value: u32) -> Self {
        Self(((id_type as u32) << Self::TYPE_SHIFT) | (value & Self::ID_MASK))
    }

    #[inline]
    pub fn id_type(&self) -> Result<IdType, &'static str> {
        IdType::try_from((self.0 >> Self::TYPE_SHIFT) as u8)
    }

    #[inline]
    pub const fn value(&self) -> u32 {
        self.0 & Self::ID_MASK
    }

    #[inline]
    pub const fn as_u32(&self) -> u32 {
        self.0
    }
}

impl From<u32> for CustomId {
    #[inline]
    fn from(raw: u32) -> Self {
        Self(raw)
    }
}

impl From<CustomId> for u32 {
    #[inline]
    fn from(custom_id: CustomId) -> Self {
        custom_id.0
    }
}

impl fmt::Debug for CustomId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let type_str = self.id_type().map_or_else(|_| "Unknown", |t| match t {
            IdType::Client => "Client",
            IdType::Server => "Server",
            IdType::Entity => "Entity",
            IdType::Ghost => "Ghost",
        });
        write!(f, "CustomId({}:{})", type_str, self.value())
    }
}