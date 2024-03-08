#![warn(clippy::all)]

mod game;
mod messages;
mod parse;
mod util;
mod viser;

use parse::file_to_match_settings;
use rlbot_sockets::{flat, flatbuffers::root, SocketDataType};
use std::{env::args, thread};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt, Result as IoResult},
    net::{TcpListener, TcpStream},
    sync::{broadcast, mpsc, oneshot},
};

const RLBOT_SOCKETS_PORT: u16 = 23234;
const DEFAULT_ADDRESS: &str = "127.0.0.1";

#[tokio::main]
async fn main() -> IoResult<()> {
    let headless = args().skip(1).any(|arg| arg == "--no-rlviser");

    let (game_tx_hold, _) = broadcast::channel(255);
    let (tx, game_rx) = mpsc::channel(255);
    let (shutdown_sender, mut shutdown_receiver) = mpsc::channel(1);

    let game_tx = game_tx_hold.clone();
    thread::spawn(move || game::run_rl(game_tx, game_rx, shutdown_sender, headless));

    let tcp_connection = TcpListener::bind(format!("{DEFAULT_ADDRESS}:{RLBOT_SOCKETS_PORT}")).await?;

    println!("Server listening on port {RLBOT_SOCKETS_PORT}");

    loop {
        tokio::select! {
            biased;
            Ok((client, _)) = tcp_connection.accept() => {
                let client_session = ClientSession::new(client, tx.clone(), game_tx_hold.subscribe());
                tokio::spawn(async move { client_session.handle_connection().await });
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
    client_params: Option<flat::ReadyMessageT>,
    buffer: Vec<u8>,
}

impl ClientSession {
    fn new(client: TcpStream, tx: mpsc::Sender<messages::ToGame>, rx: broadcast::Receiver<messages::FromGame>) -> Self {
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
                Ok(data_type) = self.client.read_u16() => {
                    if !self.handle_client_message(data_type).await? {
                        break;
                    }
                }
                Ok(msg) = self.rx.recv() => {
                    if !self.handle_game_message(msg).await? {
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

        self.buffer.extend_from_slice(&(data_type as u16).to_be_bytes());
        self.buffer
            .extend_from_slice(&u16::try_from(flat.len()).expect("Flatbuffer too large").to_be_bytes());
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
            SocketDataType::MatchSettings => {
                let match_settings = root::<flat::MatchSettings>(&self.buffer).unwrap().unpack();
                self.tx.send(messages::ToGame::MatchSettings(match_settings)).await.unwrap();
            }
            SocketDataType::ReadyMessage => {
                let ready_message = root::<flat::ReadyMessage>(&self.buffer).unwrap().unpack();
                self.client_params.replace(ready_message);

                let (match_settings_tx, match_settings_rx) = oneshot::channel();
                self.tx
                    .send(messages::ToGame::MatchSettingsRequest(match_settings_tx))
                    .await
                    .unwrap();

                if let Ok(match_settings_flat) = match_settings_rx.await {
                    self.buffered_send_flat(SocketDataType::MatchSettings, &match_settings_flat)
                        .await?;
                }

                let (field_info_tx, field_info_rx) = oneshot::channel();
                self.tx.send(messages::ToGame::FieldInfoRequest(field_info_tx)).await.unwrap();

                if let Ok(field_info_flat) = field_info_rx.await {
                    self.buffered_send_flat(SocketDataType::FieldInfo, &field_info_flat).await?;
                }
            }
            SocketDataType::StartCommand => {
                let start_command = root::<flat::StartCommand>(&self.buffer).unwrap().unpack();

                match file_to_match_settings(start_command.config_path).await {
                    Ok(match_settings) => {
                        self.tx.send(messages::ToGame::MatchSettings(match_settings)).await.unwrap();
                    }
                    Err(e) => {
                        println!("Error reading match settings: {e}");
                    }
                }
            }
            SocketDataType::PlayerInput => {
                let input = root::<flat::PlayerInput>(&self.buffer).unwrap().unpack();
                self.tx.send(messages::ToGame::PlayerInput(input)).await.unwrap();
            }
            SocketDataType::StopCommand => {
                let command = root::<flat::StopCommand>(&self.buffer).unwrap().unpack();
                self.tx.send(messages::ToGame::StopCommand(command)).await.unwrap();
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
                let Some(client_params) = &self.client_params else {
                    return Ok(false);
                };

                if force || client_params.close_after_match {
                    return Ok(false);
                }
            }
            messages::FromGame::GameTickPacket(packet) => {
                if self.client_params.is_some() {
                    self.buffered_send_flat(SocketDataType::GameTickPacket, &packet).await?;
                }
            }
            messages::FromGame::MatchSettings(settings) => {
                self.buffered_send_flat(SocketDataType::MatchSettings, &settings).await?;
            }
            messages::FromGame::FieldInfo(field) => {
                self.buffered_send_flat(SocketDataType::FieldInfo, &field).await?;
            }
        }

        Ok(true)
    }
}
