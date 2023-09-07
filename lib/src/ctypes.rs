#[allow(dead_code)]
pub enum RLBotCoreStatus {
    Success,
    BufferOverfilled,
    MessageLargerThanMax,
    InvalidNumPlayers,
    InvalidBotSkill,
    InvalidHumanIndex,
    InvalidName,
    InvalidTeam,
    InvalidTeamColorID,
    InvalidCustomColorID,
    InvalidGameValues,
    InvalidThrottle,
    InvalidSteer,
    InvalidPitch,
    InvalidYaw,
    InvalidRoll,
    InvalidPlayerIndex,
    InvalidQuickChatPreset,
    InvalidRenderType,
    QuickChatRateExceeded,
    NotInitialized,
}

#[repr(C)]
#[derive(Debug)]
pub struct PlayerInput {
    pub throttle: f32,
    pub steer: f32,
    pub pitch: f32,
    pub yaw: f32,
    pub roll: f32,
    pub jump: u8,
    pub boost: u8,
    pub handbrake: u8,
    pub use_item: u8,
}

#[repr(C)]
#[derive(Debug)]
pub struct ByteBuffer {
    pub data: *const u8,
    pub size: usize,
}

#[repr(C)]
#[derive(Debug)]
pub struct Vector3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

#[repr(C)]
#[derive(Debug)]
pub struct BoostPad {
    pub location: Vector3,
    pub is_full_boost: bool,
}

#[repr(C)]
#[derive(Debug)]
pub struct GoalInfo {
    pub team_num: u8,
    pub location: Vector3,
    pub direction: Vector3,
    pub width: f32,
    pub height: f32,
}

#[repr(C)]
#[derive(Debug)]
pub struct FieldInfoPacket {
    pub boost_pads: [BoostPad; 50],
    pub num_boosts: i32,
    pub goals: [GoalInfo; 200],
    pub num_goals: i32,
}
