use rlbot_sockets::flat;
use tokio::sync::oneshot;

#[derive(Debug)]
pub enum ToGame {
    // FieldInfoRequest,
    MatchSettingsRequest(oneshot::Sender<Box<[u8]>>),
    MatchSettings(flat::MatchSettingsT),
    // PlayerInput(flat::PlayerInputT),
    // DesiredGameState(flat::DesiredGameStateT),
    // RenderGroup(flat::RenderGroupT),
    // RemoveRenderGroup(flat::RemoveRenderGroupT),
    StopCommand(flat::StopCommandT),
}

#[derive(Clone, Debug)]
pub enum FromGame {
    None,
    GameTickPacket(Box<[u8]>),
    // QuickChat,
    // BallPrediction,
    // MessagePacket(flat::MessagePacketT),
}
