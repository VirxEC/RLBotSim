mod from_str;
#[allow(clippy::all, non_snake_case, unused_imports)]
mod generated;

pub use flatbuffers;
pub use generated::rlbot::flat;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SocketDataType {
    None,
    GameTickPacket,
    FieldInfo,
    StartCommand,
    MatchSettings,
    PlayerInput,
    DesiredGameState,
    RenderGroup,
    RemoveRenderGroup,
    QuickChat,
    BallPrediction,
    ReadyMessage,
    MessagePacket,
    StopCommand,
}

impl SocketDataType {
    #[inline]
    #[track_caller]
    pub fn from_u16(data_type: u16) -> Self {
        match data_type {
            0 => Self::None,
            1 => Self::GameTickPacket,
            2 => Self::FieldInfo,
            3 => Self::StartCommand,
            4 => Self::MatchSettings,
            5 => Self::PlayerInput,
            6 => Self::DesiredGameState,
            7 => Self::RenderGroup,
            8 => Self::RemoveRenderGroup,
            9 => Self::QuickChat,
            10 => Self::BallPrediction,
            11 => Self::ReadyMessage,
            12 => Self::MessagePacket,
            13 => Self::StopCommand,
            _ => panic!("Invalid socket data type: {}", data_type),
        }
    }
}

impl From<u16> for SocketDataType {
    #[inline]
    fn from(data_type: u16) -> Self {
        SocketDataType::from_u16(data_type)
    }
}
