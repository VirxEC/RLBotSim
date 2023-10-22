mod builders;
mod deflat;
mod game;

use deflat::ExtraInfo;
use game::{run_rl, SimMessage};
use rlbot_core_types::{flatbuffers, gen::rlbot::flat, SocketDataType};
use rocketsim_rs::{
    bytes::FromBytesExact,
    init,
    sim::{CarConfig, Team},
};
use tokio::{
    io::{self, AsyncReadExt, AsyncWriteExt, BufReader, BufWriter},
    net::{tcp::WriteHalf, TcpListener, TcpStream},
    sync::RwLock,
};

const RLBOT_SOCKETS_PORT: u16 = 23234;
pub static RLBOT_EXTRA_INFO: RwLock<ExtraInfo> = RwLock::const_new(ExtraInfo::const_default());

#[derive(Clone, Copy, Debug)]
enum NetworkingRole {
    None,
    LanClient,
    RemoteRLBotServer,
    RemoteRLBotClient,
}

impl From<u8> for NetworkingRole {
    #[inline]
    fn from(role: u8) -> Self {
        match role {
            0 => NetworkingRole::None,
            1 => NetworkingRole::LanClient,
            2 => NetworkingRole::RemoteRLBotServer,
            3 => NetworkingRole::RemoteRLBotClient,
            _ => panic!("Invalid networking role: {role}"),
        }
    }
}

#[tokio::main]
async fn main() -> io::Result<()> {
    init(None);

    let args: Vec<String> = std::env::args().collect();

    let port = &args[1];
    let address = &args[2];
    let role = NetworkingRole::from(args[3].parse::<u8>().unwrap());

    let rl_address = format!("{address}:{port}");
    println!("address: {rl_address}, role: {role:?}");

    match role {
        NetworkingRole::None => {
            let rl_address_clone = rl_address.clone();
            std::thread::spawn(|| run_rl(rl_address_clone));
        }
        _ => unimplemented!("Networking role {role:?} not implemented!"),
    }

    let tcp_connection = TcpListener::bind(format!("{address}:{RLBOT_SOCKETS_PORT}")).await?;
    // println!("Listening on {address}:{port}...");

    loop {
        let (stream, _) = tcp_connection.accept().await?;
        let rl_address_clone = rl_address.clone();
        tokio::spawn(async { handle_connection(stream, rl_address_clone).await });
    }
}

async fn handle_connection(mut bot_stream: TcpStream, rl_address: String) -> io::Result<()> {
    println!("Something connected to RLBot @ {}!", bot_stream.peer_addr()?);

    let (bot_r, bot_w) = bot_stream.split();
    let mut bot_reader = BufReader::new(bot_r);
    let mut bot_writer = BufWriter::new(bot_w);

    let mut rl_stream = TcpStream::connect(rl_address).await?;
    let (rl_r, rl_w) = rl_stream.split();
    let mut rl_writer = BufWriter::new(rl_w);
    let mut rl_reader = BufReader::new(rl_r);

    loop {
        tokio::select! {
            msg = bot_reader.read_u16() => {
                let msg_type = msg?;

                let n = bot_reader.read_u16().await? as usize;

                if n == 0 {
                    break;
                }

                let mut bytes = vec![0; n];
                bot_reader.read_exact(&mut bytes).await?;

                match SocketDataType::from(msg_type) {
                    SocketDataType::FieldInfo => handle_field_info(bytes, &mut rl_writer).await?,
                    SocketDataType::MatchSettings => handle_match_settings(bytes, &mut rl_writer).await?,
                    SocketDataType::ReadyMessage => handle_ready_message(bytes, &mut rl_writer)?,
                    SocketDataType::PlayerInput => handle_player_input(bytes, &mut rl_writer).await?,
                    data_type => unimplemented!("Data type {data_type:?} not implemented!"),
                }
            }
            Ok(msg) = rl_reader.read_u16() => {
                let n = rl_reader.read_u16().await?;

                if n == 0 {
                    break;
                }

                let mut bytes = vec![0; n as usize + 4];
                bytes[..2].copy_from_slice(&msg.to_be_bytes());
                bytes[2..4].copy_from_slice(&n.to_be_bytes());
                rl_reader.read_exact(&mut bytes[4..]).await?;

                bot_writer.write_all(&bytes).await?;
            }
        }
    }

    Ok(())
}

async fn write_bytes(rl_writer: &mut BufWriter<WriteHalf<'_>>, bytes: Vec<u8>) -> io::Result<()> {
    rl_writer.write_u16(u16::try_from(bytes.len()).unwrap()).await?;
    rl_writer.write_all(&bytes).await
}

async fn handle_player_input(bytes: Vec<u8>, rl_writer: &mut BufWriter<WriteHalf<'_>>) -> io::Result<()> {
    if bytes.len() == 1 {
        unimplemented!("Getting a player's input is not implemented!");
    }

    write_bytes(rl_writer, SimMessage::SetPlayerInput(bytes).to_bytes()).await?;
    rl_writer.flush().await?;
    Ok(())
}

async fn handle_field_info(bytes: Vec<u8>, rl_writer: &mut BufWriter<WriteHalf<'_>>) -> io::Result<()> {
    if bytes.len() == 1 {
        write_bytes(rl_writer, vec![5]).await?;
        rl_writer.flush().await?;
        return Ok(());
    }

    unimplemented!("Setting the FieldInfo is not implemented!");
}

async fn handle_match_settings(bytes: Vec<u8>, rl_writer: &mut BufWriter<WriteHalf<'_>>) -> io::Result<()> {
    if bytes.len() == 1 {
        write_bytes(rl_writer, vec![4]).await?;
        rl_writer.flush().await?;
        return Ok(());
    }

    let match_settings = flatbuffers::root::<flat::MatchSettings>(&bytes).unwrap();

    // create the required SimChange messages from match_settings
    write_bytes(rl_writer, SimMessage::Reset.to_bytes()).await?;

    let mut extra_info = RLBOT_EXTRA_INFO.write().await;

    if let Some(players) = match_settings.playerConfigurations() {
        extra_info.car_info.clear();
        extra_info.car_info.reserve(players.len());

        for car in players {
            write_bytes(
                rl_writer,
                SimMessage::AddCar((Team::from_bytes(&[car.team() as u8]), Box::new(*CarConfig::octane()))).to_bytes(),
            )
            .await?;

            extra_info.car_info.push(car.into());
        }
    }

    write_bytes(rl_writer, SimMessage::MatchSettings(bytes).to_bytes()).await?;
    write_bytes(rl_writer, SimMessage::Kickoff.to_bytes()).await?;

    rl_writer.flush().await?;

    Ok(())
}

fn handle_ready_message(bytes: Vec<u8>, _rl_writer: &mut BufWriter<WriteHalf<'_>>) -> io::Result<()> {
    let ready_message = flatbuffers::root::<flat::ReadyMessage>(&bytes).unwrap();

    let _wants_quick_chat = ready_message.wantsQuickChat();
    let _wants_game_messages = ready_message.wantsGameMessages();
    let _wants_ball_predictions = ready_message.wantsBallPredictions();

    Ok(())
}
