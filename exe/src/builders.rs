use crate::{
    game::{BLUE_TEAM_SCORE, ORANGE_TEAM_SCORE},
    RLBOT_EXTRA_INFO,
};
use glam::Mat3;
use rlbot_core_types::{flatbuffers::FlatBufferBuilder, gen::rlbot::flat, SocketDataType};
use rocketsim_rs::{
    math::{Angle, Vec3},
    sim::MutatorConfig,
    GameState,
};
use std::sync::atomic::Ordering;
use tokio::io::AsyncWriteExt;

#[inline]
fn build_vector3_flat(vector: Vec3) -> flat::Vector3 {
    flat::Vector3::new(vector.x, vector.y, vector.z)
}

#[inline]
fn build_rotator_flat(angle: Angle) -> flat::Rotator {
    flat::Rotator::new(angle.pitch, angle.yaw, angle.roll)
}

pub async fn build_fi_flat(game_pads: impl Iterator<Item = (bool, Vec3)> + '_) -> Vec<u8> {
    let mut builder = FlatBufferBuilder::new();

    let pads = game_pads
        .into_iter()
        .map(|(is_full_boost, pos)| {
            flat::BoostPad::create(
                &mut builder,
                &flat::BoostPadArgs {
                    isFullBoost: is_full_boost,
                    location: Some(&build_vector3_flat(pos)),
                },
            )
        })
        .collect::<Vec<_>>();

    let goals = [
        flat::GoalInfo::create(
            &mut builder,
            &flat::GoalInfoArgs {
                teamNum: 0,
                location: Some(&build_vector3_flat(Vec3::new(0., -5120., 642.775))),
                direction: Some(&build_vector3_flat(Vec3::new(0., 1., 0.))),
                width: 892.755,
                height: 642.775,
            },
        ),
        flat::GoalInfo::create(
            &mut builder,
            &flat::GoalInfoArgs {
                teamNum: 1,
                location: Some(&build_vector3_flat(Vec3::new(0., 5120., 642.775))),
                direction: Some(&build_vector3_flat(Vec3::new(0., -1., 0.))),
                width: 892.755,
                height: 642.775,
            },
        ),
    ];

    let fi_args = flat::FieldInfoArgs {
        boostPads: Some(builder.create_vector(&pads)),
        goals: Some(builder.create_vector(&goals)),
    };

    let fi = flat::FieldInfo::create(&mut builder, &fi_args);
    builder.finish(fi, None);
    let data = builder.finished_data();

    let mut vec = Vec::with_capacity(4 + data.len());
    vec.write_u16(SocketDataType::FieldInfo as u16).await.unwrap();
    vec.write_u16(u16::try_from(data.len()).unwrap()).await.unwrap();
    vec.extend_from_slice(data);
    vec
}

pub struct GameInfo {
    pub seconds_elapsed: f32,
    pub game_time_remaining: f32,
    pub game_speed: f32,
    pub is_overtime: bool,
    pub is_unlimited_time: bool,
    pub is_round_active: bool,
    pub is_kickoff_pause: bool,
    pub is_match_ended: bool,
}

pub async fn build_gtp_flat(game_state: GameState, mutators: MutatorConfig, game_info: GameInfo) -> Vec<u8> {
    let mut builder = FlatBufferBuilder::new();

    let players = game_state
        .cars
        .into_iter()
        .zip(&RLBOT_EXTRA_INFO.read().await.car_info)
        .map(|(car, extra)| {
            let physics = flat::Physics::create(
                &mut builder,
                &flat::PhysicsArgs {
                    location: Some(&build_vector3_flat(car.state.pos)),
                    velocity: Some(&build_vector3_flat(car.state.vel)),
                    angularVelocity: Some(&build_vector3_flat(car.state.ang_vel)),
                    rotation: Some(&build_rotator_flat(Angle::from_rotmat(car.state.rot_mat))),
                },
            );

            let name = builder.create_string(&extra.name);

            let hitbox = flat::BoxShape::create(
                &mut builder,
                &flat::BoxShapeArgs {
                    length: car.config.hitbox_size.x,
                    width: car.config.hitbox_size.y,
                    height: car.config.hitbox_size.z,
                },
            );

            let hitbox_offset = build_vector3_flat(car.config.hitbox_pos_offset);

            flat::PlayerInfo::create(
                &mut builder,
                &flat::PlayerInfoArgs {
                    physics: Some(physics),
                    scoreInfo: None,
                    isDemolished: car.state.is_demoed,
                    hasWheelContact: car.state.has_contact,
                    isSupersonic: car.state.is_supersonic,
                    isBot: true,
                    jumped: car.state.has_jumped,
                    doubleJumped: car.state.has_double_jumped,
                    name: Some(name),
                    team: car.team as i32,
                    boost: car.state.boost as i32,
                    hitbox: Some(hitbox),
                    hitboxOffset: Some(&hitbox_offset),
                    spawnId: extra.spawn_id,
                },
            )
        })
        .collect::<Vec<_>>();

    let boost_pad_states = game_state
        .pads
        .into_iter()
        .map(|pad| {
            flat::BoostPadState::create(
                &mut builder,
                &flat::BoostPadStateArgs {
                    isActive: pad.state.is_active,
                    timer: pad.state.cooldown,
                },
            )
        })
        .collect::<Vec<_>>();

    let ball_physics = flat::Physics::create(
        &mut builder,
        &flat::PhysicsArgs {
            location: Some(&build_vector3_flat(game_state.ball.pos)),
            velocity: Some(&build_vector3_flat(game_state.ball.vel)),
            angularVelocity: Some(&build_vector3_flat(game_state.ball.ang_vel)),
            rotation: Some(&build_rotator_flat(Angle::from(&Mat3::from(game_state.ball.rot_mat)))),
        },
    );

    let ball_shape = flat::SphereShape::create(
        &mut builder,
        &flat::SphereShapeArgs {
            diameter: mutators.ball_radius * 2.,
        },
    );

    let ball = flat::BallInfo::create(
        &mut builder,
        &flat::BallInfoArgs {
            physics: Some(ball_physics),
            latestTouch: None,
            dropShotInfo: None,
            shape: Some(ball_shape.as_union_value()),
            shape_type: flat::CollisionShape::SphereShape,
        },
    );

    let game_info = flat::GameInfo::create(
        &mut builder,
        &flat::GameInfoArgs {
            secondsElapsed: game_info.seconds_elapsed,
            frameNum: game_state.tick_count as i32,
            gameTimeRemaining: game_info.game_time_remaining,
            gameSpeed: game_info.game_speed,
            isOvertime: game_info.is_overtime,
            isUnlimitedTime: game_info.is_unlimited_time,
            isRoundActive: game_info.is_round_active,
            isKickoffPause: game_info.is_kickoff_pause,
            isMatchEnded: game_info.is_match_ended,
            worldGravityZ: mutators.gravity.z,
        },
    );

    let teams = [
        flat::TeamInfo::create(
            &mut builder,
            &flat::TeamInfoArgs {
                teamIndex: 0,
                score: BLUE_TEAM_SCORE.load(Ordering::SeqCst) as i32,
            },
        ),
        flat::TeamInfo::create(
            &mut builder,
            &flat::TeamInfoArgs {
                teamIndex: 1,
                score: ORANGE_TEAM_SCORE.load(Ordering::SeqCst) as i32,
            },
        ),
    ];

    let gtp_args = flat::GameTickPacketArgs {
        players: Some(builder.create_vector(&players)),
        boostPadStates: Some(builder.create_vector(&boost_pad_states)),
        ball: Some(ball),
        gameInfo: Some(game_info),
        tileInformation: None,
        teams: Some(builder.create_vector(&teams)),
    };

    let gtp = flat::GameTickPacket::create(&mut builder, &gtp_args);
    builder.finish(gtp, None);
    let data = builder.finished_data();

    let mut vec = Vec::with_capacity(4 + data.len());
    vec.write_u16(SocketDataType::GameTickPacket as u16).await.unwrap();
    vec.write_u16(u16::try_from(data.len()).unwrap()).await.unwrap();
    vec.extend_from_slice(data);
    vec
}
