use bytes::{Buf, Bytes};
use mathtools::Vec2;

pub trait SpatialServerBinaryPacket: Sized {
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

pub trait BinaryField {
    const SIZE: usize;
    fn read_from(data: &mut Bytes) -> Self;
    fn write_to(&self, buf: &mut Vec<u8>);
}

impl BinaryField for u32 {
    const SIZE: usize = 4;

    #[inline]
    fn read_from(data: &mut Bytes) -> Self {
        data.get_u32_le()
    }

    #[inline]
    fn write_to(&self, buf: &mut Vec<u8>) {
        buf.extend_from_slice(&self.to_le_bytes());
    }
}

impl BinaryField for Vec2<f32> {
    const SIZE: usize = 8;

    #[inline]
    fn read_from(data: &mut Bytes) -> Self {
        Vec2::new(data.get_f32_le(), data.get_f32_le())
    }

    #[inline]
    fn write_to(&self, buf: &mut Vec<u8>) {
        buf.extend_from_slice(&self.x.to_le_bytes());
        buf.extend_from_slice(&self.y.to_le_bytes());
    }
}

impl BinaryField for u16 {
    const SIZE: usize = 2;

    #[inline]
    fn read_from(data: &mut Bytes) -> Self {
        data.get_u16_le()
    }

    #[inline]
    fn write_to(&self, buf: &mut Vec<u8>) {
        buf.extend_from_slice(&self.to_le_bytes());
    }
}

impl BinaryField for Vec<u8>{
    const SIZE: usize = 0;

    #[inline]
    fn read_from(data: &mut Bytes) -> Self {
        let len = data.remaining();
        let mut buf = vec![0; len];
        data.copy_to_slice(&mut buf);
        buf
    }

    #[inline]
    fn write_to(&self, buf: &mut Vec<u8>) {
        buf.extend_from_slice(self);
    }
}

impl<const N: usize> BinaryField for [u8; N] {
    const SIZE: usize = N;

    #[inline]
    fn read_from(data: &mut Bytes) -> Self {
        let mut arr = [0; N];
        data.copy_to_slice(&mut arr);
        arr
    }

    #[inline]
    fn write_to(&self, buf: &mut Vec<u8>) {
        buf.extend_from_slice(self);
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
        impl SpatialServerBinaryPacket for $struct_name {
            const TAG: u8 = $tag;

            // Calcul de la taille : 1 octet (TAG) + somme des tailles des champs
            const PACKET_SIZE: usize = 1 $( + <$field_type as BinaryField>::SIZE )*;

            #[inline]
            fn parse_payload(data: &mut bytes::Bytes) -> Option<Self> {
                Some(Self {
                    $( $field_name: <$field_type as BinaryField>::read_from(data), )*
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