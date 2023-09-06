mod deflat;
mod game;
mod generated;

use deflat::ExtraInfo;
use game::{run_rl, SimMessage};
use generated::rlbot_generated::rlbot::flat;

use rocketsim_rs::{
    bytes::{FromBytesExact, ToBytes},
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
            _ => panic!("Invalid networking role: {}", role),
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
    // println!("Something connected to RLBot @ {}!", stream.peer_addr()?);

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
                    SocketDataType::MatchSettings => {
                        send_match_settings(flatbuffers::root::<flat::MatchSettings>(&bytes).unwrap(), &mut rl_writer).await?;
                    }
                    SocketDataType::ReadyMessage => {
                        handle_ready_message(flatbuffers::root::<flat::ReadyMessage>(&bytes).unwrap(), &mut rl_writer).await?;
                    }
                    data_type => {dbg!(data_type);},
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

#[derive(Clone, Copy, Debug)]
enum SocketDataType {
    GameTickPacket = 1,
    FieldInfo,
    MatchSettings,
    PlayerInput,
    ActorMapping,
    ComputerId,
    DesiredGameState,
    RenderGroup,
    QuickChat,
    BallPrediction,
    ReadyMessage,
    MessagePacket,
}

impl SocketDataType {
    #[inline]
    fn from_u16(data_type: u16) -> Self {
        match data_type {
            1 => SocketDataType::GameTickPacket,
            2 => SocketDataType::FieldInfo,
            3 => SocketDataType::MatchSettings,
            4 => SocketDataType::PlayerInput,
            5 => SocketDataType::ActorMapping,
            6 => SocketDataType::ComputerId,
            7 => SocketDataType::DesiredGameState,
            8 => SocketDataType::RenderGroup,
            9 => SocketDataType::QuickChat,
            10 => SocketDataType::BallPrediction,
            11 => SocketDataType::ReadyMessage,
            12 => SocketDataType::MessagePacket,
            _ => panic!("Invalid socket data type: {}", data_type),
        }
    }
}

impl From<u16> for SocketDataType {
    #[inline]
    fn from(data_type: u16) -> Self {
        SocketDataType::from_u16(data_type)
    }
}

async fn write_bytes(rl_writer: &mut BufWriter<WriteHalf<'_>>, bytes: Vec<u8>) -> io::Result<()> {
    rl_writer.write_u16(bytes.len() as u16).await?;
    rl_writer.write_all(&bytes).await
}

async fn send_match_settings(
    match_settings: flat::MatchSettings<'_>,
    rl_writer: &mut BufWriter<WriteHalf<'_>>,
) -> io::Result<()> {
    // create the required SimChange messages from match_settings
    write_bytes(rl_writer, SimMessage::Reset.to_bytes()).await?;

    let mut extra_info = RLBOT_EXTRA_INFO.write().await;

    if let Some(players) = match_settings.playerConfigurations() {
        extra_info.car_info.clear();
        extra_info.car_info.reserve(players.len());

        for car in players {
            write_bytes(
                rl_writer,
                SimMessage::AddCar((Team::from_bytes(&[car.team() as u8]), *CarConfig::octane())).to_bytes(),
            )
            .await?;

            extra_info.car_info.push(car.into());
        }
    }

    write_bytes(rl_writer, SimMessage::Kickoff.to_bytes()).await?;

    rl_writer.flush().await?;

    Ok(())
}

async fn handle_ready_message(
    ready_message: flat::ReadyMessage<'_>,
    _rl_connection: &mut BufWriter<WriteHalf<'_>>,
) -> io::Result<()> {
    let _wants_quick_chat = ready_message.wantsQuickChat();
    let _wants_game_messages = ready_message.wantsGameMessages();
    let _wants_ball_predictions = ready_message.wantsBallPredictions();

    Ok(())
}
