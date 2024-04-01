use rlbot_sockets::flat;
use tokio::sync::oneshot;

#[derive(Debug)]
pub enum ToGame {
    FieldInfoRequest(oneshot::Sender<Box<[u8]>>),
    MatchSettingsRequest(oneshot::Sender<Box<[u8]>>),
    MatchSettings(flat::MatchSettingsT),
    PlayerInput(flat::PlayerInputT),
    DesiredGameState(flat::DesiredGameStateT),
    // RenderGroup(flat::RenderGroupT),
    // RemoveRenderGroup(flat::RemoveRenderGroupT),
    StopCommand(flat::StopCommandT),
}

#[derive(Clone, Debug)]
pub enum FromGame {
    StopCommand(bool),
    GameTickPacket(Box<[u8]>),
    MatchSettings(Box<[u8]>),
    FieldInfo(Box<[u8]>),
    // QuickChat,
    BallPrediction(Box<[u8]>),
    // MessagePacket(flat::MessagePacketT),
}
