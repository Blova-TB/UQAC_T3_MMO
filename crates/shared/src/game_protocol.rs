use bitcode::{Encode, Decode};

#[derive(Debug, Clone, Encode, Decode)]
pub struct PlayerInputPayload {
    pub x: f32,
    pub y: f32,
    pub keys: u8,
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct WorldSyncPayload {
    pub entities: Vec<(u32, (f32, f32))>,
}

macro_rules! define_game_protocol {
    (
        $( $variant:ident($stream_id:expr) => $payload:ty ),* $(,)?
    ) => {

        #[repr(u16)]
        #[derive(Debug, Copy, Clone, PartialEq, Eq)]
        pub enum LogicalStream {
            $( $variant = $stream_id, )*
        }

        impl TryFrom<u16> for LogicalStream {
            type Error = &'static str;

            fn try_from(value: u16) -> Result<Self, Self::Error> {
                match value {
                    $( $stream_id => Ok(LogicalStream::$variant), )*
                    _ => Err("Stream logique inconnu"),
                }
            }
        }

        #[derive(Debug, Encode, Decode)]
        pub enum GameMessage {
            $( $variant($payload), )*
        }

        impl GameMessage {
            pub fn decode(raw_stream_id: u16, payload_data: &[u8]) -> Option<Self> {
                let logical_id = raw_stream_id >> 2;
                let logical_stream = LogicalStream::try_from(logical_id).ok()?;
                match logical_stream {
                    $(
                        LogicalStream::$variant => {
                            let data: $payload = bitcode::decode(payload_data).ok()?;
                            Some(GameMessage::$variant(data))
                        }
                    )*
                }
            }
        }
    };
}

define_game_protocol! {
    Input(1)     => PlayerInputPayload,
    WorldSync(2) => WorldSyncPayload,
}