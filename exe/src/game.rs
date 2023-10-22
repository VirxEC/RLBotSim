use crate::builders::{build_fi_flat, build_gtp_flat, GameInfo};
use rlbot_core_types::{flatbuffers, gen::rlbot::flat, SocketDataType};
use rocketsim_rs::{
    bytes::{FromBytesExact, ToBytes, ToBytesExact},
    sim::{Arena, CarConfig, CarControls, Team},
};
use std::{
    net::SocketAddr,
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};
use tokio::{
    io::{self, AsyncReadExt, AsyncWriteExt, BufReader, BufWriter},
    net::{TcpListener, TcpStream, UdpSocket},
    sync::{mpsc, watch},
    time,
};

#[derive(Debug)]
pub enum SimMessage {
    Reset,
    AddCar((Team, Box<CarConfig>)),
    Kickoff,
    MatchSettings(Vec<u8>),
    WantMatchSettings(u64),
    WantFieldInfo(u64),
    SetPlayerInput(Vec<u8>),
}

impl SimMessage {
    pub fn from_bytes(bytes: &[u8]) -> Self {
        match bytes[0] {
            0 => Self::Reset,
            1 => {
                let car_config = CarConfig::from_bytes(&bytes[2..]);
                Self::AddCar((Team::from_bytes(&bytes[1..]), Box::new(car_config)))
            }
            2 => Self::Kickoff,
            3 => Self::MatchSettings(bytes[1..].to_vec()),
            4 => Self::WantMatchSettings(u64::from_bytes(&bytes[1..])),
            5 => Self::WantFieldInfo(u64::from_bytes(&bytes[1..])),
            6 => Self::SetPlayerInput(bytes[1..].to_vec()),
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
            Self::SetPlayerInput(data) => {
                bytes.reserve(1 + data.len());
                bytes.push(6);
                bytes.extend_from_slice(data);
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

pub static BLUE_TEAM_SCORE: AtomicU64 = AtomicU64::new(0);
pub static ORANGE_TEAM_SCORE: AtomicU64 = AtomicU64::new(0);

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
                    arena.pin_mut().reset_to_random_kickoff(None);
                },
                SimMessage::MatchSettings(data) => {
                    match_settings_msg.clear();
                    match_settings_msg.reserve(4 + data.len());
                    match_settings_msg.write_u16(SocketDataType::MatchSettings as u16).await.unwrap();
                    match_settings_msg.write_u16(u16::try_from(data.len()).unwrap()).await.unwrap();
                    match_settings_msg.extend_from_slice(&data);
                }
                SimMessage::WantMatchSettings(client_id) => {
                    client_msg_txs[client_id as usize].send(match_settings_msg.clone()).await.unwrap();
                }
                SimMessage::WantFieldInfo(client_id) => {
                    client_msg_txs[client_id as usize].send(build_fi_flat(arena.iter_pad_static()).await).await.unwrap();
                }
                SimMessage::SetPlayerInput(data) => {
                    let player_input = flatbuffers::root::<flat::PlayerInput<'_>>(&data).unwrap();
                    let index = player_input.playerIndex() as usize;
                    let controller_state = player_input.controllerState().unwrap();
                    let car_controls = CarControls {
                        throttle: controller_state.throttle(),
                        steer: controller_state.steer(),
                        pitch: controller_state.pitch(),
                        yaw: controller_state.yaw(),
                        roll: controller_state.roll(),
                        jump: controller_state.jump(),
                        boost: controller_state.boost(),
                        handbrake: controller_state.handbrake(),
                    };

                    let car_id = arena.pin_mut().get_cars()[index];

                    if let Err(e) = arena.pin_mut().set_car_controls(car_id, car_controls) {
                        eprintln!("Failed to set car controls: {e}");
                    }
                }
            },
            _ = tick.tick() => {
                if is_round_active {
                    arena.pin_mut().step(1);
                }

                let game_state = arena.pin_mut().get_game_state();
                let rlviser_bytes = game_state.to_bytes();

                let latest_touch_tick_count = game_state.cars.iter().filter_map(|car| {
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
