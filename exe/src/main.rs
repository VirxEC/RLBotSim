#![forbid(unsafe_code)]

mod agent_res;
mod game;
mod messages;
mod parse;
mod util;
mod viser;

use clap::{Parser, Subcommand};
use parse::file_to_match_settings;
use rlbot_sockets::{flat, flatbuffers::root, SocketDataType};
use std::{net::Ipv4Addr, path::Path, thread};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt, Result as IoResult},
    net::{TcpListener, TcpStream},
    sync::{broadcast, mpsc, oneshot},
};

const RLVISER_PATH: &str = if cfg!(windows) {
    "./rlviser.exe"
} else {
    "./rlviser"
};

const RLBOT_PORT: u16 = 23234;
const RLVISER_PORT: u16 = 23235;
const ROCKETSIM_PORT: u16 = 23236;

fn valid_path(s: &str) -> Result<String, String> {
    if Path::new(s).exists() {
        Ok(s.to_string())
    } else {
        Err(format!("Path `{s}` does not exist"))
    }
}

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    commands: Option<Commands>,
    #[arg(long, value_parser = clap::value_parser!(u16).range(1..), default_value_t = RLBOT_PORT)]
    rlbot_port: u16,
}

#[derive(Subcommand)]
pub enum Commands {
    #[command(name = "rlviser")]
    RLViser {
        #[arg(long, value_parser = valid_path, default_value = RLVISER_PATH)]
        rlviser_path: String,
        #[arg(long, value_parser = clap::value_parser!(u16).range(1..), default_value_t = RLVISER_PORT)]
        rlviser_port: u16,
        #[arg(long, value_parser = clap::value_parser!(u16).range(1..), default_value_t = ROCKETSIM_PORT)]
        rocketsim_port: u16,
    },
    Headless,
}

impl Default for Commands {
    fn default() -> Self {
        Commands::RLViser {
            rlviser_path: RLVISER_PATH.to_string(),
            rlviser_port: RLVISER_PORT,
            rocketsim_port: ROCKETSIM_PORT,
        }
    }
}

#[tokio::main]
async fn main() -> IoResult<()> {
    let cli = Cli::parse();

    let (game_tx_hold, _) = broadcast::channel(63);
    let (tx, game_rx) = mpsc::channel(31);
    let (shutdown_sender, mut shutdown_receiver) = mpsc::channel(1);

    let game_tx = game_tx_hold.clone();

    if cli.commands.is_none() {
        assert!(
            Path::new(RLVISER_PATH).exists(),
            "Path `{RLVISER_PATH}` does not exist"
        );
    }

    thread::spawn(move || {
        game::run_rl(
            game_tx,
            game_rx,
            shutdown_sender,
            cli.rlbot_port,
            cli.commands.unwrap_or_default(),
        )
    });

    let tcp_connection = TcpListener::bind((Ipv4Addr::new(0, 0, 0, 0), cli.rlbot_port)).await?;
    println!("Server listening on port {}", cli.rlbot_port);

    loop {
        tokio::select! {
            biased;
            Ok((client, _)) = tcp_connection.accept() => {
                client.set_nodelay(true)?;
                let client_session = ClientSession::new(client, tx.clone(), game_tx_hold.subscribe());
                tokio::spawn(async move {
                    if let Err(e) = client_session.handle_connection().await {
                        println!("Error from client connection: {e}");
                    }
                });
            }
            _ = shutdown_receiver.recv() => {
                break;
            }
        }
    }

    Ok(())
}

struct ClientSession {
    client: TcpStream,
    tx: mpsc::Sender<messages::ToGame>,
    rx: broadcast::Receiver<messages::FromGame>,
    client_params: Option<flat::ConnectionSettingsT>,
    buffer: Vec<u8>,
}

impl ClientSession {
    #[inline]
    fn new(
        client: TcpStream,
        tx: mpsc::Sender<messages::ToGame>,
        rx: broadcast::Receiver<messages::FromGame>,
    ) -> Self {
        Self {
            client,
            tx,
            rx,
            client_params: None,
            buffer: Vec::with_capacity(1024),
        }
    }

    async fn handle_connection(mut self) -> IoResult<()> {
        loop {
            tokio::select! {
                biased;
                Ok(msg) = self.rx.recv() => {
                    if !self.handle_game_message(msg).await.is_ok_and(|x| x) {
                        break;
                    }
                }
                Ok(data_type) = self.client.read_u16() => {
                    if !self.handle_client_message(data_type).await.is_ok_and(|x| x) {
                        break;
                    }
                }
                else => break,
            }
        }

        println!("Client exiting loop and closing connection");
        self.buffered_send_flat(SocketDataType::None, &[1]).await?;

        Ok(())
    }

    async fn buffered_send_flat(&mut self, data_type: SocketDataType, flat: &[u8]) -> IoResult<()> {
        self.buffer.clear();
        self.buffer.reserve(4 + flat.len());

        self.buffer
            .extend_from_slice(&(data_type as u16).to_be_bytes());
        let size = u16::try_from(flat.len()).expect("Flatbuffer too large");
        self.buffer.extend_from_slice(&size.to_be_bytes());
        self.buffer.extend_from_slice(flat);

        self.client.write_all(&self.buffer).await?;
        self.client.flush().await?;
        Ok(())
    }

    async fn handle_client_message(&mut self, data_type: u16) -> IoResult<bool> {
        let size = self.client.read_u16().await?;

        self.buffer.resize(usize::from(size), 0);
        self.client.read_exact(&mut self.buffer).await?;

        match SocketDataType::from_u16(data_type) {
            SocketDataType::None => {
                println!("Received None message type, closing connection");
                return Ok(false);
            }
            SocketDataType::MatchConfig => {
                let match_settings = root::<flat::MatchConfiguration>(&self.buffer)
                    .unwrap()
                    .unpack();
                self.tx
                    .blocking_send(messages::ToGame::MatchSettings(match_settings))
                    .unwrap();
            }
            SocketDataType::ConnectionSettings => {
                let connection_settings = root::<flat::ConnectionSettings>(&self.buffer)
                    .unwrap()
                    .unpack();

                let agent_id = connection_settings.agent_id.clone();
                self.client_params.replace(connection_settings);

                let (match_settings_tx, match_settings_rx) = oneshot::channel();
                self.tx
                    .send(messages::ToGame::MatchSettingsRequest(match_settings_tx))
                    .await
                    .unwrap();

                if let Ok(match_settings_flat) = match_settings_rx.await {
                    self.buffered_send_flat(SocketDataType::MatchConfig, &match_settings_flat)
                        .await?;
                }

                let (field_info_tx, field_info_rx) = oneshot::channel();
                self.tx
                    .send(messages::ToGame::FieldInfoRequest(field_info_tx))
                    .await
                    .unwrap();

                if let Ok(field_info_flat) = field_info_rx.await {
                    self.buffered_send_flat(SocketDataType::FieldInfo, &field_info_flat)
                        .await?;
                }

                let (controllable_team_info_tx, controllable_team_info_rx) = oneshot::channel();
                self.tx
                    .send(messages::ToGame::ControllableTeamInfoRequest(
                        agent_id,
                        controllable_team_info_tx,
                    ))
                    .await
                    .unwrap();

                if let Ok(Some(controllable_team_info_flat)) = controllable_team_info_rx.await {
                    self.buffered_send_flat(
                        SocketDataType::ControllableTeamInfo,
                        &controllable_team_info_flat,
                    )
                    .await?;
                }
            }
            SocketDataType::StartCommand => {
                let start_command = root::<flat::StartCommand>(&self.buffer).unwrap().unpack();

                match file_to_match_settings(start_command.config_path).await {
                    Ok(match_settings) => {
                        self.tx
                            .send(messages::ToGame::MatchSettings(match_settings))
                            .await
                            .unwrap();
                    }
                    Err(e) => {
                        println!("Error reading match settings: {e}");
                    }
                }
            }
            SocketDataType::PlayerInput => {
                let input = root::<flat::PlayerInput>(&self.buffer).unwrap().unpack();
                self.tx
                    .send(messages::ToGame::PlayerInput(input))
                    .await
                    .unwrap();
            }
            SocketDataType::DesiredGameState => {
                let desired_state = root::<flat::DesiredGameState>(&self.buffer)
                    .unwrap()
                    .unpack();
                self.tx
                    .send(messages::ToGame::DesiredGameState(desired_state))
                    .await
                    .unwrap();
            }
            SocketDataType::RenderGroup => {
                let group = root::<flat::RenderGroup>(&self.buffer).unwrap().unpack();
                self.tx
                    .send(messages::ToGame::RenderGroup(group))
                    .await
                    .unwrap();
            }
            SocketDataType::RemoveRenderGroup => {
                let group = root::<flat::RemoveRenderGroup>(&self.buffer)
                    .unwrap()
                    .unpack();
                self.tx
                    .send(messages::ToGame::RemoveRenderGroup(group))
                    .await
                    .unwrap();
            }
            SocketDataType::MatchComm => {
                // assert that it's actually a MatchComm message
                assert!(root::<flat::MatchComm>(&self.buffer).is_ok());

                self.tx
                    .send(messages::ToGame::MatchComm(
                        self.buffer.clone().into_boxed_slice(),
                    ))
                    .await
                    .unwrap();
            }
            SocketDataType::StopCommand => {
                let command = root::<flat::StopCommand>(&self.buffer).unwrap().unpack();
                self.tx
                    .send(messages::ToGame::StopCommand(command))
                    .await
                    .unwrap();
            }
            i => {
                println!("Received message type: {i:?}");
            }
        }

        Ok(true)
    }

    async fn handle_game_message(&mut self, msg: messages::FromGame) -> IoResult<bool> {
        match msg {
            messages::FromGame::StopCommand(force) => {
                return Ok(force
                    || self
                        .client_params
                        .as_ref()
                        .is_some_and(|x| x.close_between_matches));
            }
            messages::FromGame::GameTickPacket(packet) => {
                if self.client_params.is_some() {
                    self.buffered_send_flat(SocketDataType::GamePacket, &packet)
                        .await?;
                }
            }
            messages::FromGame::MatchSettings(settings) => {
                self.buffered_send_flat(SocketDataType::MatchConfig, &settings)
                    .await?;
            }
            messages::FromGame::FieldInfo(field) => {
                self.buffered_send_flat(SocketDataType::FieldInfo, &field)
                    .await?;
            }
            messages::FromGame::MatchComm(message) => {
                let Some(client_params) = &self.client_params else {
                    return Ok(true);
                };

                if client_params.wants_comms {
                    self.buffered_send_flat(SocketDataType::MatchComm, &message)
                        .await?;
                }
            }
            messages::FromGame::BallPrediction(prediction) => {
                let Some(client_params) = &self.client_params else {
                    return Ok(true);
                };

                if client_params.wants_ball_predictions {
                    self.buffered_send_flat(SocketDataType::BallPrediction, &prediction)
                        .await?;
                }
            }
        }

        Ok(true)
    }
}
