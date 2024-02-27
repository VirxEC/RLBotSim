#![warn(clippy::all)]

mod game;

use rlbot_core_types::{
    flat::{self, MatchSettingsT},
    flatbuffers, RustMessage, SocketDataType,
};
use std::thread;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt, Result as IoResult},
    net::{TcpListener, TcpStream},
    sync::{broadcast, mpsc},
};

const RLBOT_SOCKETS_PORT: u16 = 23234;
const DEFAULT_ADDRESS: &str = "127.0.0.1";

#[tokio::main]
async fn main() -> IoResult<()> {
    let (game_tx_hold, _) = broadcast::channel(255);
    let (tx, game_rx) = mpsc::channel(255);
    let (shutdown_sender, mut shutdown_receiver) = mpsc::channel(1);

    let game_tx = game_tx_hold.clone();
    thread::spawn(move || game::run_rl(game_tx, game_rx, shutdown_sender));

    let tcp_connection = TcpListener::bind(format!("{DEFAULT_ADDRESS}:{RLBOT_SOCKETS_PORT}")).await?;

    println!("Server listening on port {RLBOT_SOCKETS_PORT}");

    loop {
        tokio::select! {
            Ok((stream, _)) = tcp_connection.accept() => {
                let tx_2 = tx.clone();
                let rx = game_tx_hold.subscribe();
                tokio::spawn(async move { handle_connection(stream, tx_2, rx).await });
            }
            _ = shutdown_receiver.recv() => {
                break;
            }
        }
    }

    Ok(())
}

async fn handle_connection(
    mut client: TcpStream,
    tx: mpsc::Sender<RustMessage>,
    mut rx: broadcast::Receiver<RustMessage>,
) -> IoResult<()> {
    let mut client_params = None;
    let mut buffer = Vec::with_capacity(512);

    loop {
        tokio::select! {
            Ok(data_type) = client.read_u16() => {
                if !handle_client_message(data_type, &mut client, &tx, &mut buffer, &mut client_params).await? {
                    break;
                }
            }
            Ok(msg) = rx.recv() => {
                if !handle_game_message(msg, &mut client, &mut client_params).await? {
                    break;
                }
            }
            else => break,
        }
    }

    println!("Client exiting loop and closing connection");

    Ok(())
}

async fn handle_client_message(
    data_type: u16,
    client: &mut TcpStream,
    tx: &mpsc::Sender<RustMessage>,
    buffer: &mut Vec<u8>,
    client_params: &mut Option<flat::ReadyMessageT>,
) -> IoResult<bool> {
    let size = client.read_u16().await?;

    buffer.resize(usize::from(size), 0);
    client.read_exact(buffer).await?;

    match SocketDataType::from_u16(data_type) {
        SocketDataType::None => {
            println!("Received None message type, closing connection");
            return Ok(false);
        }
        SocketDataType::MatchSettings => {
            let match_settings = flatbuffers::root::<flat::MatchSettings>(buffer).unwrap().unpack();
            tx.send(RustMessage::MatchSettings(match_settings)).await.unwrap();
        }
        SocketDataType::ReadyMessage => {
            let ready_message = flatbuffers::root::<flat::ReadyMessage>(buffer).unwrap().unpack();
            client_params.replace(ready_message);
        }
        SocketDataType::StartCommand => {
            let start_command = flatbuffers::root::<flat::StartCommand>(buffer).unwrap().unpack();
            let match_settings_path = start_command.config_path;
            println!("Match settings path: {match_settings_path}");

            tx.send(RustMessage::MatchSettings(MatchSettingsT::default())).await.unwrap();
        }
        SocketDataType::StopCommand => {
            let command = flatbuffers::root::<flat::StopCommand>(buffer).unwrap().unpack();
            tx.send(RustMessage::StopCommand(command)).await.unwrap();
        }
        i => {
            println!("Received message type: {i:?}");
        }
    }

    Ok(true)
}

async fn handle_game_message(
    msg: RustMessage,
    client: &mut TcpStream,
    client_params: &mut Option<flat::ReadyMessageT>,
) -> IoResult<bool> {
    match msg {
        RustMessage::None => {
            println!("Received None message type, closing connection");
            return Ok(false);
        }
        RustMessage::GameTickPacket(packet) => {
            let Some(params) = client_params.as_ref() else {
                return Ok(true);
            };

            client.write_u16(SocketDataType::GameTickPacket as u16).await?;
            client.write_u16(packet.len() as u16).await?;
            client.write_all(&packet).await?;
        }
        i => {
            println!("Received message type: {i:?}");
        }
    }

    Ok(true)
}
