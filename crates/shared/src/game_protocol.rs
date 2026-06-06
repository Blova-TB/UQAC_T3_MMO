use bitcode::{Encode, Decode};

// --- Constantes d'Input (Bitmask) ---
pub const INPUT_UP: u8     = 1 << 0; // 0000 0001 (Avancer)
pub const INPUT_DOWN: u8   = 1 << 1; // 0000 0010 (Reculer)
pub const INPUT_LEFT: u8   = 1 << 2; // 0000 0100 (Gauche)
pub const INPUT_RIGHT: u8  = 1 << 3; // 0000 1000 (Droite)
pub const INPUT_ACTION: u8 = 1 << 4; // 0001 0000 (Action 'E')


#[derive(Debug, Clone, Encode, Decode)]
pub struct PlayerInputPayload {
    pub inputs : [PlayerInput; 16],
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct PlayerInput {
    pub input: u8
}

impl PlayerInput {
    pub fn is_up(&self) -> bool {
        self.input & INPUT_UP != 0
    }

    pub fn is_down(&self) -> bool {
        self.input & INPUT_DOWN != 0
    }

    pub fn is_left(&self) -> bool {
        self.input & INPUT_LEFT != 0
    }

    pub fn is_right(&self) -> bool {
        self.input & INPUT_RIGHT != 0
    }

    pub fn is_action(&self) -> bool {
        self.input & INPUT_ACTION != 0
    }
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