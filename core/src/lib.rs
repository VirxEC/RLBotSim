mod generated;

pub use flatbuffers;
pub use generated::rlbot_generated as gen;

#[repr(u16)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SocketDataType {
    GameTickPacket = 1,
    FieldInfo,
    MatchSettings,
    PlayerInput,
    ActorMapping,
    ComputerId,
    DesiredGameState,
    RenderGroup,
    QuickChat,
    BallPrediction,
    ReadyMessage,
    MessagePacket,
}

impl SocketDataType {
    #[inline]
    pub fn from_u16(data_type: u16) -> Self {
        match data_type {
            1 => SocketDataType::GameTickPacket,
            2 => SocketDataType::FieldInfo,
            3 => SocketDataType::MatchSettings,
            4 => SocketDataType::PlayerInput,
            5 => SocketDataType::ActorMapping,
            6 => SocketDataType::ComputerId,
            7 => SocketDataType::DesiredGameState,
            8 => SocketDataType::RenderGroup,
            9 => SocketDataType::QuickChat,
            10 => SocketDataType::BallPrediction,
            11 => SocketDataType::ReadyMessage,
            12 => SocketDataType::MessagePacket,
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
