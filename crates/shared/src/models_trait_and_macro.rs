use bytes::{Buf, Bytes};
use mathtools::Vec2;
use crate::custom_id::CustomId;

pub trait ServerBinaryPacket: Sized {
    const TAG: u8;
    const PACKET_SIZE: usize;

    fn parse_payload(data: &mut Bytes) -> Option<Self>;
    fn write_payload(&self, buf: &mut Vec<u8>);

    fn try_from_bytes(mut data: Bytes) -> Option<Self> {
        if data.len() < Self::PACKET_SIZE {
            return None;
        }

        if data.get_u8() != Self::TAG {
            return None;
        }

        Self::parse_payload(&mut data)
    }

    fn to_bytes(&self) -> Bytes {
        let mut buf = Vec::with_capacity(Self::PACKET_SIZE);
        buf.push(Self::TAG);
        self.write_payload(&mut buf);
        Bytes::from(buf)
    }
}

pub trait BinaryField : Sized {
    const MIN_SIZE: usize;
    fn try_read_from(data: &mut Bytes) -> Option<Self>;
    fn write_to(&self, buf: &mut Vec<u8>);
}

impl BinaryField for u32 {
    const MIN_SIZE: usize = 4;

    #[inline]
    fn try_read_from(data: &mut Bytes) -> Option<Self> {
        if data.remaining() < Self::MIN_SIZE {
            return None;
        }
        Some(data.get_u32_le())
    }

    #[inline]
    fn write_to(&self, buf: &mut Vec<u8>) {
        buf.extend_from_slice(&self.to_le_bytes());
    }
}

impl BinaryField for u16 {
    const MIN_SIZE: usize = 2;

    #[inline]
    fn try_read_from(data: &mut Bytes) -> Option<Self> {
        if data.remaining() < Self::MIN_SIZE {
            return None;
        }
        Some(data.get_u16_le())
    }

    #[inline]
    fn write_to(&self, buf: &mut Vec<u8>) {
        buf.extend_from_slice(&self.to_le_bytes());
    }
}

impl BinaryField for u8 {
    const MIN_SIZE: usize = 1;

    #[inline]
    fn try_read_from(data: &mut Bytes) -> Option<Self> {
        if data.remaining() < Self::MIN_SIZE {
            return None;
        }
        Some(data.get_u8())
    }

    #[inline]
    fn write_to(&self, buf: &mut Vec<u8>) {
        buf.push(*self);
    }
}

impl BinaryField for Vec2<f32> {
    const MIN_SIZE: usize = 8;

    #[inline]
    fn try_read_from(data: &mut Bytes) -> Option<Self> {
        if data.remaining() < Self::MIN_SIZE {
            return None;
        }
        Some(Vec2::new(data.get_f32_le(), data.get_f32_le()))
    }

    #[inline]
    fn write_to(&self, buf: &mut Vec<u8>) {
        buf.extend_from_slice(&self.x.to_le_bytes());
        buf.extend_from_slice(&self.y.to_le_bytes());
    }
}

impl<T: BinaryField> BinaryField for Vec<T> {
    const MIN_SIZE: usize = 4; // Taille du préfixe (u32)

    #[inline]
    fn try_read_from(data: &mut Bytes) -> Option<Self>{

        if data.remaining() < Self::MIN_SIZE {
            return None;
        }

        let len = data.get_u32_le() as usize;

        if data.remaining() < len.saturating_mul(T::MIN_SIZE) {
            return None;
        }

        let mut vec = Vec::with_capacity(len);
        for _ in 0..len {
            vec.push(T::try_read_from(data)?);
        }
        Some(vec)
    }

    #[inline]
    fn write_to(&self, buf: &mut Vec<u8>) {
        buf.extend_from_slice(&(self.len() as u32).to_le_bytes());
        for item in self {
            item.write_to(buf);
        }
    }
}

impl<const N: usize> BinaryField for [u8; N] {
    const MIN_SIZE: usize = N;

    #[inline]
    fn try_read_from(data: &mut Bytes) -> Option<Self> {
        if data.remaining() < Self::MIN_SIZE {
            return None;
        }
        let mut arr = [0; N];
        data.copy_to_slice(&mut arr);
        Some(arr)
    }

    #[inline]
    fn write_to(&self, buf: &mut Vec<u8>) {
        buf.extend_from_slice(self);
    }
}

pub struct PlayerData {
    pub client_id: CustomId,
    pub pos: Vec2<f32>,
}

impl BinaryField for PlayerData {
    const MIN_SIZE: usize = CustomId::MIN_SIZE + <Vec2<f32> as BinaryField>::MIN_SIZE;

    #[inline]
    fn try_read_from(data: &mut Bytes) -> Option<Self> {
        Some(Self {
            client_id: CustomId::try_read_from(data)?,
            pos: Vec2::try_read_from(data)?,
        })
    }

    #[inline]
    fn write_to(&self, buf: &mut Vec<u8>) {
        self.client_id.write_to(buf);
        self.pos.write_to(buf);
    }
}

#[macro_export]
macro_rules! define_packet {
    (
        // Syntaxe attendue : struct NomDuPaquet(TAG) { champ: Type, ... }
        $struct_name:ident($tag:expr) {
            $( $field_name:ident : $field_type:ty ),* $(,)?
        }
    ) => {
        // 1. Génération de la structure
        pub struct $struct_name {
            $( pub $field_name: $field_type, )*
        }

        // 2. Implémentation du trait principal
        impl ServerBinaryPacket for $struct_name {
            const TAG: u8 = $tag;
            const PACKET_SIZE: usize = 1 $( + <$field_type as BinaryField>::MIN_SIZE )*;

            #[inline]
            fn parse_payload(data: &mut bytes::Bytes) -> Option<Self> {
                Some(Self {
                    $( $field_name: <$field_type as BinaryField>::try_read_from(data)?, )*
                })
            }

            #[inline]
            fn write_payload(&self, buf: &mut Vec<u8>) {
                $( self.$field_name.write_to(buf); )*
            }
        }
    };
}

#[macro_export]
macro_rules! define_packet_router {
    (
        $vis:vis enum $enum_name:ident {
            // Accepte une liste de variantes sous la forme `NomVariante(TypeStructure)`
            $( $variant:ident($packet_type:ty) ),* $(,)?
        }
    ) => {
        // 1. Génération de l'Enum
        $vis enum $enum_name {
            $( $variant($packet_type), )*
        }

        // 2. Génération de l'implémentation de routage
        impl $enum_name {
            pub fn try_from_bytes(data: bytes::Bytes) -> Option<Self> {
                if data.is_empty() {
                    return None;
                }

                let tag = data[0];

                match tag {
                    $(
                        // Utilise les constantes associées du trait BinaryPacket
                        <$packet_type>::TAG => {
                            <$packet_type>::try_from_bytes(data).map(Self::$variant)
                        }
                    )*
                    _ => None,
                }
            }
        }
    };
}