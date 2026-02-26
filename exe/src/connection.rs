use std::{
    net::{AddrParseError, IpAddr, Ipv6Addr, SocketAddr},
    time::Duration,
};

use async_timer::new_timer;
use rlbot_flat::{
    flat::{CoreMessage, CorePacket, InterfaceMessage, InterfacePacket, InterfacePacketRef},
    planus::{self, ReadAsRoot},
};
use thiserror::Error;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

enum GenericMessage {
    InterfaceMessage(InterfaceMessage),
    CoreMessage(CoreMessage),
}

impl From<InterfaceMessage> for GenericMessage {
    fn from(value: InterfaceMessage) -> Self {
        GenericMessage::InterfaceMessage(value)
    }
}
impl From<CoreMessage> for GenericMessage {
    fn from(value: CoreMessage) -> Self {
        GenericMessage::CoreMessage(value)
    }
}

#[derive(Error, Debug)]
pub enum PacketBuildError {
    #[error("Payload too large {0}, couldn't fit in u16")]
    PayloadTooLarge(usize),
}

fn build_packet_payload(
    packet: impl Into<GenericMessage>,
    builder: &mut planus::Builder,
) -> Result<Vec<u8>, PacketBuildError> {
    builder.clear();
    let payload = match packet.into() {
        GenericMessage::InterfaceMessage(x) => {
            let packet: InterfacePacket = x.into();
            builder.finish(packet, None)
        }
        GenericMessage::CoreMessage(x) => {
            let packet: CorePacket = x.into();
            builder.finish(packet, None)
        }
    };

    let data_len_bin = u16::try_from(payload.len())
        .map_err(|_| PacketBuildError::PayloadTooLarge(payload.len()))?
        .to_be_bytes()
        .to_vec();
    Ok([data_len_bin, payload.to_vec()].concat())
}

#[derive(Error, Debug)]
pub enum PacketParseError {
    #[error("Unpacking flatbuffer failed")]
    InvalidFlatbuffer(#[from] planus::Error),
}

#[derive(Error, Debug)]
pub enum RLBotError {
    #[error("Failed to connect to RLBot on given port")]
    Connection(#[from] std::io::Error),
    #[error("Parsing packet failed")]
    PacketParseError(#[from] PacketParseError),
    #[error("Building packet failed")]
    PacketBuildError(#[from] PacketBuildError),
    #[error("Invalid address, cannot parse")]
    InvalidAddrError(#[from] AddrParseError),
}

pub struct RLBotConnection {
    pub(crate) stream: TcpStream,
    builder: planus::Builder,
    recv_buf: Box<[u8; u16::MAX as usize]>,
}

impl RLBotConnection {
    const LOCALHOST: IpAddr = IpAddr::V6(Ipv6Addr::LOCALHOST);

    async fn send_packet_enum(&mut self, packet: CoreMessage) -> Result<(), RLBotError> {
        self.stream
            .write_all(&build_packet_payload(packet, &mut self.builder)?)
            .await?;
        self.stream.flush().await?;
        Ok(())
    }

    pub async fn send_packet(&mut self, packet: impl Into<CoreMessage>) -> Result<(), RLBotError> {
        self.send_packet_enum(packet.into()).await
    }

    pub async fn recv_packet(&mut self) -> Result<InterfaceMessage, RLBotError> {
        let mut buf = [0u8; 2];

        self.stream.read_exact(&mut buf).await?;

        let data_len = u16::from_be_bytes(buf);

        let buf = &mut self.recv_buf[0..data_len as usize];

        self.stream.read_exact(buf).await?;

        let packet_ref: InterfacePacketRef =
            InterfacePacketRef::read_as_root(buf).map_err(PacketParseError::InvalidFlatbuffer)?;
        let packet: InterfacePacket = packet_ref.try_into().unwrap();

        Ok(packet.message)
    }

    /// Establish a new connection to core
    pub async fn new(rlbot_port: u16) -> Result<Self, RLBotError> {
        let stream = TcpStream::connect(SocketAddr::new(Self::LOCALHOST, rlbot_port)).await?;
        stream.set_nodelay(true)?;

        Ok(Self {
            stream,
            builder: planus::Builder::with_capacity(1024),
            recv_buf: Box::new([0u8; u16::MAX as usize]),
        })
    }
}

pub async fn connect_rlbot(rlbot_port: u16) -> RLBotConnection {
    const NUM_REATTEMPTS: u32 = 4;

    println!("Connecting to RLBot on port {rlbot_port}.");

    let mut attempts = 0;

    loop {
        match RLBotConnection::new(rlbot_port).await {
            Ok(conn) => break conn,
            Err(err) => {
                attempts += 1;
                if attempts >= NUM_REATTEMPTS {
                    panic!(
                        "Failed to connect to RLBot (attempt {attempts}/{NUM_REATTEMPTS}): {err}"
                    );
                } else {
                    eprintln!(
                        "Failed to connect to RLBot (attempt {attempts}/{NUM_REATTEMPTS}): {err}"
                    );
                }

                new_timer(Duration::from_millis(250 * 2u64.pow(attempts))).await;
            }
        }
    }
}
