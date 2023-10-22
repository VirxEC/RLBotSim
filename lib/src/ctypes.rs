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

impl PlayerInput {
    /// f32 is 4 bytes * 5 = 20 bytes
    /// u8 is 1 byte * 4 = 4 bytes
    pub const NUM_BYTES: usize = 4 * 5 + 4;
}

#[repr(C)]
#[derive(Debug)]
pub struct ByteBuffer {
    pub data: *mut u8,
    pub size: usize,
}

#[repr(C)]
#[derive(Debug)]
pub struct Vector3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

// class Rotator(Struct):
//     _fields_ = [("pitch", ctypes.c_float),
//                 ("yaw", ctypes.c_float),
//                 ("roll", ctypes.c_float)]
#[repr(C)]
#[derive(Debug)]
pub struct Rotator {
    pub pitch: f32,
    pub yaw: f32,
    pub roll: f32,
}

// class Physics(Struct):
//     _fields_ = [("location", Vector3),
//                 ("rotation", Rotator),
//                 ("velocity", Vector3),
//                 ("angular_velocity", Vector3)]
#[repr(C)]
#[derive(Debug)]
pub struct Physics {
    pub location: Vector3,
    pub rotation: Rotator,
    pub velocity: Vector3,
    pub angular_velocity: Vector3,
}

// class Touch(Struct):
//     _fields_ = [("player_name", ctypes.c_wchar * MAX_NAME_LENGTH),
//                 ("time_seconds", ctypes.c_float),
//                 ("hit_location", Vector3),
//                 ("hit_normal", Vector3),
//                 ("team", ctypes.c_int),
//                 ("player_index", ctypes.c_int)]
#[repr(C)]
#[derive(Debug)]
pub struct Touch {
    pub player_name: [i32; 32],
    pub time_seconds: f32,
    pub hit_location: Vector3,
    pub hit_normal: Vector3,
    pub team: i32,
    pub player_index: i32,
}

// class ScoreInfo(Struct):
//     # Describes the points for a single player (see TeamInfo for team scores)
//     _fields_ = [("score", ctypes.c_int),
//                 ("goals", ctypes.c_int),
//                 ("own_goals", ctypes.c_int),
//                 ("assists", ctypes.c_int),
//                 ("saves", ctypes.c_int),
//                 ("shots", ctypes.c_int),
//                 ("demolitions", ctypes.c_int)]
#[repr(C)]
#[derive(Debug)]
pub struct ScoreInfo {
    pub score: i32,
    pub goals: i32,
    pub own_goals: i32,
    pub assists: i32,
    pub saves: i32,
    pub shots: i32,
    pub demolitions: i32,
}

// class BoxShape(Struct):
//     _fields_ = [("length", ctypes.c_float),
//                 ("width", ctypes.c_float),
//                 ("height", ctypes.c_float)]
#[repr(C)]
#[derive(Debug)]
pub struct BoxShape {
    pub length: f32,
    pub width: f32,
    pub height: f32,
}

// class SphereShape(Struct):
//     _fields_ = [("diameter", ctypes.c_float)]
#[repr(C)]
#[derive(Debug)]
pub struct SphereShape {
    pub diameter: f32,
}

// class CylinderShape(Struct):
//     _fields_ = [("diameter", ctypes.c_float),
//                 ("height", ctypes.c_float)]
#[repr(C)]
#[derive(Debug)]
pub struct CylinderShape {
    pub diameter: f32,
    pub height: f32,
}

// class ShapeType(IntEnum):
//     box = 0
//     sphere = 1
//     cylinder = 2
#[repr(i32)]
#[derive(Debug)]
pub enum ShapeType {
    Box = 0,
    Sphere = 1,
    Cylinder = 2,
}

// class CollisionShape(Struct):
//     _fields_ = [("type", ctypes.c_int),
//                 ("box", BoxShape),
//                 ("sphere", SphereShape),
//                 ("cylinder", CylinderShape)]
#[repr(C)]
#[derive(Debug)]
pub struct CollisionShape {
    pub shape_type: ShapeType,
    pub box_shape: BoxShape,
    pub sphere_shape: SphereShape,
    pub cylinder_shape: CylinderShape,
}

// class PlayerInfo(Struct):
//     _fields_ = [("physics", Physics),
//                 ("score_info", ScoreInfo),
//                 ("is_demolished", ctypes.c_bool),
//                 # True if your wheels are on the ground, the wall, or the ceiling. False if you're midair or turtling.
//                 ("has_wheel_contact", ctypes.c_bool),
//                 ("is_super_sonic", ctypes.c_bool),
//                 ("is_bot", ctypes.c_bool),
//                 # True if the player has jumped. Falling off the ceiling / driving off the goal post does not count.
//                 ("jumped", ctypes.c_bool),
//                 # True if player has double jumped. False does not mean you have a jump remaining, because the
//                 # aerial timer can run out, and that doesn't affect this flag.
//                 ("double_jumped", ctypes.c_bool),
//                 ("name", ctypes.c_wchar * MAX_NAME_LENGTH),
//                 ("team", ctypes.c_ubyte),
//                 ("boost", ctypes.c_int),
//                 ("hitbox", BoxShape),
//                 ("hitbox_offset", Vector3),
//                 ("spawn_id", ctypes.c_int)]
#[repr(C)]
#[derive(Debug)]
pub struct PlayerInfo {
    pub physics: Physics,
    pub score_info: ScoreInfo,
    pub is_demolished: bool,
    pub has_wheel_contact: bool,
    pub is_super_sonic: bool,
    pub is_bot: bool,
    pub jumped: bool,
    pub double_jumped: bool,
    pub name: [i32; 32],
    pub team: u8,
    pub boost: i32,
    pub hitbox: BoxShape,
    pub hitbox_offset: Vector3,
    pub spawn_id: i32,
}

// class DropShotInfo(Struct):
//     _fields_ = [("absorbed_force", ctypes.c_float),
//                 ("damage_index", ctypes.c_int),
//                 ("force_accum_recent", ctypes.c_float)]
#[repr(C)]
#[derive(Debug)]
pub struct DropShotInfo {
    pub absorbed_force: f32,
    pub damage_index: i32,
    pub force_accum_recent: f32,
}

// class BallInfo(Struct):
//     _fields_ = [("physics", Physics),
//                 ("latest_touch", Touch),
//                 ("drop_shot_info", DropShotInfo),
//                 ("collision_shape", CollisionShape)]
#[repr(C)]
#[derive(Debug)]
pub struct BallInfo {
    pub physics: Physics,
    pub latest_touch: Touch,
    pub drop_shot_info: DropShotInfo,
    pub collision_shape: CollisionShape,
}

// class BoostPadState(Struct):
// _fields_ = [("is_active", ctypes.c_bool),
//             ("timer", ctypes.c_float)]
#[repr(C)]
#[derive(Debug)]
pub struct BoostPadState {
    pub is_active: bool,
    pub timer: f32,
}

// class TileInfo(Struct):
// _fields_ = [("tile_state", ctypes.c_int)]  # see DropShotTileState
#[repr(C)]
#[derive(Debug)]
pub struct TileInfo {
    pub tile_state: i32,
}

// class GameInfo(Struct):
//     _fields_ = [("seconds_elapsed", ctypes.c_float),
//                 ("game_time_remaining", ctypes.c_float),
//                 ("is_overtime", ctypes.c_bool),
//                 ("is_unlimited_time", ctypes.c_bool),
//                 # True when cars are allowed to move, and during the pause menu. False during replays.
//                 ("is_round_active", ctypes.c_bool),
//                 # Only false during a kickoff, when the car is allowed to move, and the ball has not been hit,
//                 # and the game clock has not started yet. If both players sit still, game clock will eventually
//                 # start and this will become true.
//                 ("is_kickoff_pause", ctypes.c_bool),
//                 # Turns true after final replay, the moment the 'winner' screen appears. Remains true during next match
//                 # countdown. Turns false again the moment the 'choose team' screen appears.
//                 ("is_match_ended", ctypes.c_bool),
//                 ("world_gravity_z", ctypes.c_float),
//                 # Game speed multiplier, 1.0 is regular game speed.
//                 ("game_speed", ctypes.c_float),
//                 # Number of physics frames which have elapsed in the game.
//                 # May increase by more than one across consecutive packets.
//                 ("frame_num", ctypes.c_int)]
#[repr(C)]
#[derive(Debug)]
pub struct GameInfo {
    pub seconds_elapsed: f32,
    pub game_time_remaining: f32,
    pub is_overtime: bool,
    pub is_unlimited_time: bool,
    pub is_round_active: bool,
    pub is_kickoff_pause: bool,
    pub is_match_ended: bool,
    pub world_gravity_z: f32,
    pub game_speed: f32,
    pub frame_num: i32,
}

// class TeamInfo(Struct):
//     _fields_ = [("team_index", ctypes.c_int),
//                 ("score", ctypes.c_int)]
#[repr(C)]
#[derive(Debug)]
pub struct TeamInfo {
    pub team_index: i32,
    pub score: i32,
}

// class GameTickPacket(Struct):
//     _fields_ = [("game_cars", PlayerInfo * MAX_PLAYERS),
//                 ("num_cars", ctypes.c_int),
//                 ("game_boosts", BoostPadState * MAX_BOOSTS),
//                 ("num_boost", ctypes.c_int),
//                 ("game_ball", BallInfo),
//                 ("game_info", GameInfo),
//                 ("dropshot_tiles", TileInfo * MAX_TILES),
//                 ("num_tiles", ctypes.c_int),
//                 ("teams", TeamInfo * MAX_TEAMS),
//                 ("num_teams", ctypes.c_int)]
#[repr(C)]
#[derive(Debug)]
pub struct GameTickPacket {
    pub game_cars: [PlayerInfo; 64],
    pub num_cars: i32,
    pub game_boosts: [BoostPadState; 50],
    pub num_boost: i32,
    pub game_ball: BallInfo,
    pub game_info: GameInfo,
    pub dropshot_tiles: [TileInfo; 200],
    pub num_tiles: i32,
    pub teams: [TeamInfo; 2],
    pub num_teams: i32,
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

// class Color(Struct):
//     _fields_ = [("r", ctypes.c_ubyte),
//                 ("g", ctypes.c_ubyte),
//                 ("b", ctypes.c_ubyte),
//                 ("a", ctypes.c_ubyte)]

#[repr(C)]
#[derive(Debug)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}
