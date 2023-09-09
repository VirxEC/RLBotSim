use std::{
    net::SocketAddr,
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use glam::Quat;
use rlbot_core_types::{flatbuffers::FlatBufferBuilder, gen::rlbot::flat, SocketDataType};
use tokio::{
    io::{self, AsyncReadExt, AsyncWriteExt, BufReader, BufWriter},
    net::{TcpListener, TcpStream, UdpSocket},
    sync::{mpsc, watch},
    time,
};

use rocketsim_rs::{
    bytes::{FromBytesExact, ToBytes, ToBytesExact},
    math::{Angle, Vec3},
    sim::{Arena, CarConfig, MutatorConfig, Team},
    GameState,
};

use crate::RLBOT_EXTRA_INFO;

#[derive(Debug)]
pub enum SimMessage {
    Reset,
    AddCar((Team, CarConfig)),
    Kickoff,
    MatchSettings(Vec<u8>),
    WantMatchSettings(u64),
    WantFieldInfo(u64),
}

impl SimMessage {
    pub fn from_bytes(bytes: &[u8]) -> Self {
        match bytes[0] {
            0 => Self::Reset,
            1 => {
                let car_config = CarConfig::from_bytes(&bytes[2..]);
                Self::AddCar((Team::from_bytes(&bytes[1..]), car_config))
            }
            2 => Self::Kickoff,
            3 => Self::MatchSettings(bytes[1..].to_vec()),
            4 => Self::WantMatchSettings(u64::from_bytes(&bytes[1..])),
            5 => Self::WantFieldInfo(u64::from_bytes(&bytes[1..])),
            change_type => unimplemented!("SimChange type {change_type} not implemented!"),
        }
    }
}

impl SimMessage {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        match self {
            Self::Reset => bytes.push(0),
            Self::AddCar((team, car_config)) => {
                bytes.reserve(2 + CarConfig::NUM_BYTES);
                bytes.push(1);
                bytes.push(*team as u8);
                bytes.extend_from_slice(&car_config.to_bytes());
            }
            Self::Kickoff => bytes.push(2),
            Self::MatchSettings(data) => {
                bytes.reserve(1 + data.len());
                bytes.push(3);
                bytes.extend_from_slice(data);
            }
            Self::WantMatchSettings(client_id) => {
                bytes.reserve(1 + 4);
                bytes.push(4);
                bytes.extend_from_slice(&client_id.to_le_bytes());
            }
            Self::WantFieldInfo(client_id) => {
                bytes.reserve(1 + 4);
                bytes.push(5);
                bytes.extend_from_slice(&client_id.to_le_bytes());
            }
        }
        bytes
    }
}

const RLVISER_PATH: &str = if cfg!(windows) { "./rlviser.exe" } else { "./rlviser" };

async fn init() -> io::Result<(UdpSocket, SocketAddr)> {
    // launch RLViser
    if let Err(e) = Command::new(RLVISER_PATH).spawn() {
        eprintln!("Failed to launch RLViser ({RLVISER_PATH}): {e}");
    }

    // Connect to RLViser
    let socket = UdpSocket::bind("0.0.0.0:34254").await?;

    println!("Waiting for connection to socket...");
    let mut buf = [0; 1];
    let (_, src) = socket.recv_from(&mut buf).await?;

    if buf[0] == 1 {
        println!("Connection established to {src}");
    }

    // socket.set_nonblocking(true).await?;

    Ok((socket, src))
}

#[repr(u8)]
enum UdpPacketTypes {
    Quit,
    GameState,
}

struct GameInfo {
    seconds_elapsed: f32,
    game_time_remaining: f32,
    game_speed: f32,
    is_overtime: bool,
    is_unlimited_time: bool,
    is_round_active: bool,
    is_kickoff_pause: bool,
    is_match_ended: bool,
}

static BLUE_TEAM_SCORE: AtomicU64 = AtomicU64::new(0);
static ORANGE_TEAM_SCORE: AtomicU64 = AtomicU64::new(0);

#[tokio::main]
pub async fn run_rl(address: String) -> io::Result<()> {
    let tcp = TcpListener::bind(address).await?;
    let (sim_msg_tx, mut sim_msg_rx) = mpsc::channel(128);
    let (msg_distrib_tx, msg_distrib_rx) = watch::channel(Vec::new());
    let mut client_msg_txs = Vec::new();

    let mut match_settings_msg = Vec::new();
    let mut tick = time::interval(Duration::from_secs_f32(1. / 120.));
    let mut arena = Arena::default_standard();
    arena.pin_mut().set_goal_scored_callback(
        |arena, team, _user_data| {
            match team {
                Team::BLUE => BLUE_TEAM_SCORE.fetch_add(1, Ordering::SeqCst),
                Team::ORANGE => ORANGE_TEAM_SCORE.fetch_add(1, Ordering::SeqCst),
            };

            arena.reset_to_random_kickoff(None);
        },
        0,
    );

    let (socket, src) = init().await?;

    let mut is_round_active = false;
    let mut kickoff_tick_count = 0;
    let game_time_seconds = 5. * 60.;

    loop {
        tokio::select! {
            Ok((stream, _)) = tcp.accept() => {
                let tx_clone = sim_msg_tx.clone();
                let rx_clone = msg_distrib_rx.clone();
                let (client_only_tx, client_only_rx) = mpsc::channel(16);
                let client_id = client_msg_txs.len();
                client_msg_txs.push(client_only_tx);
                tokio::spawn(async move {
                    handle_rl_request(stream, tx_clone, rx_clone, client_only_rx, (client_id as u64).to_ne_bytes()).await
                });
            }
            Some(msg) = sim_msg_rx.recv() => match msg {
                SimMessage::Reset => { arena = Arena::default_standard(); is_round_active = false; },
                SimMessage::AddCar((team, car_config)) => { let _ = arena.pin_mut().add_car(team, &car_config);},
                SimMessage::Kickoff => {
                    is_round_active = true;
                    kickoff_tick_count = arena.get_tick_count();
                    arena.pin_mut().reset_to_random_kickoff(None)
                },
                SimMessage::MatchSettings(data) => {
                    match_settings_msg.clear();
                    match_settings_msg.reserve(4 + data.len());
                    match_settings_msg.write_u16(SocketDataType::MatchSettings as u16).await.unwrap();
                    match_settings_msg.write_u16(data.len() as u16).await.unwrap();
                    match_settings_msg.extend_from_slice(&data);
                }
                SimMessage::WantMatchSettings(client_id) => {
                    client_msg_txs[client_id as usize].send(match_settings_msg.clone()).await.unwrap();
                }
                SimMessage::WantFieldInfo(client_id) => {
                    client_msg_txs[client_id as usize].send(build_fi_flat(arena.iter_pad_static()).await).await.unwrap();
                }
            },
            _ = tick.tick() => {
                if is_round_active {
                    // TODO: set car controls!
                    arena.pin_mut().step(1);
                }

                let game_state = arena.pin_mut().get_game_state();
                let rlviser_bytes = game_state.to_bytes();

                let latest_touch_tick_count = game_state.cars.iter().flat_map(|car| {
                    if car.state.ball_hit_info.is_valid {
                        Some((car.id, car.state.ball_hit_info.tick_count_when_hit))
                    } else {
                        None
                    }
                }).max_by_key(|(_, tick_count)| *tick_count).map(|(id, _)| id);

                let is_kickoff_pause = latest_touch_tick_count.map(u64::from).unwrap_or_default() < kickoff_tick_count;
                let seconds_elapsed = game_state.tick_count as f32 / game_state.tick_rate;
                let is_overtime = seconds_elapsed > game_time_seconds;
                let is_match_ended = is_overtime && BLUE_TEAM_SCORE.load(Ordering::SeqCst) != ORANGE_TEAM_SCORE.load(Ordering::SeqCst);

                if is_match_ended {
                    is_round_active = false;
                }

                let game_info = GameInfo {
                    seconds_elapsed,
                    game_time_remaining: game_time_seconds - seconds_elapsed,
                    game_speed: 1.,
                    is_overtime,
                    is_unlimited_time: false,
                    is_round_active,
                    is_kickoff_pause,
                    is_match_ended,
                };

                let mutator_config = arena.get_mutator_config();
                msg_distrib_tx.send(build_gtp_flat(game_state, mutator_config, game_info).await).unwrap();

                socket.send_to(&[UdpPacketTypes::GameState as u8], src).await?;
                socket.send_to(&rlviser_bytes, src).await?;
            }
        }
    }
}

async fn handle_rl_request(
    mut stream: TcpStream,
    tx: mpsc::Sender<SimMessage>,
    mut rx: watch::Receiver<Vec<u8>>,
    mut only_rx: mpsc::Receiver<Vec<u8>>,
    client_id: [u8; 8],
) -> io::Result<()> {
    println!("Something connected to RocketSim @ {}!", stream.peer_addr()?);

    let (r, w) = stream.split();
    let mut reader = BufReader::new(r);
    let mut writer = BufWriter::new(w);

    loop {
        tokio::select! {
            msg = reader.read_u16() => {
                let num_bytes = msg?;
                if num_bytes == 0 {
                    return Ok(());
                }

                let mut buf = vec![0; num_bytes as usize];
                reader.read_exact(&mut buf).await?;

                if buf[0] >= 4 {
                    buf.extend_from_slice(&client_id)
                }

                tx.send(SimMessage::from_bytes(&buf)).await.expect("failed to send SimChange");
            }
            Some(msg) = only_rx.recv() => {
                writer.write_all(&msg).await?;
                writer.flush().await?;
            }
            Ok(()) = rx.changed() => {
                let msg = rx.borrow().clone();
                writer.write_all(&msg).await?;
                writer.flush().await?;
            }
        }
    }
}

#[inline]
fn build_vector3_flat(vector: Vec3) -> flat::Vector3 {
    flat::Vector3::new(vector.x, vector.y, vector.z)
}

#[inline]
fn build_rotator_flat(angle: Angle) -> flat::Rotator {
    flat::Rotator::new(angle.pitch, angle.yaw, angle.roll)
}

async fn build_fi_flat(game_pads: impl Iterator<Item = (bool, Vec3)> + '_) -> Vec<u8> {
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
    vec.write_u16(data.len() as u16).await.unwrap();
    vec.extend_from_slice(data);
    vec
}

async fn build_gtp_flat(game_state: GameState, mutators: MutatorConfig, game_info: GameInfo) -> Vec<u8> {
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
            rotation: Some(&build_rotator_flat(Angle::from(Quat::from_array(game_state.ball_rot)))),
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
    vec.write_u16(data.len() as u16).await.unwrap();
    vec.extend_from_slice(data);
    vec
}
