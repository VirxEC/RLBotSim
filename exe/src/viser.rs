use rocketsim_rs::{
    bytes::{FromBytes, ToBytes},
    GameState,
};
use std::{
    io::Result as IoResult,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    process::Command,
};
use tokio::net::UdpSocket;

const RLVISER_PATH: &str = if cfg!(windows) { "./rlviser.exe" } else { "./rlviser" };
const RLVISER_PORT: u16 = 45243;
const ROCKETSIM_PORT: u16 = 34254;

const RLVISER_ADDR: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), RLVISER_PORT);

#[repr(u8)]
pub enum UdpPacketTypes {
    Quit,
    GameState,
}

pub struct ExternalManager {
    socket: UdpSocket,
    buffer: Vec<u8>,
}

impl ExternalManager {
    pub async fn new() -> IoResult<Self> {
        Command::new(RLVISER_PATH).env("CARGO_MANIFEST_DIR", "").spawn()?;

        Ok(Self {
            socket: UdpSocket::bind(("0.0.0.0", ROCKETSIM_PORT)).await?,
            buffer: Vec::with_capacity(1024),
        })
    }

    pub async fn send_game_state(&mut self, game_state: &GameState) -> IoResult<()> {
        self.socket.send_to(&[UdpPacketTypes::GameState as u8], RLVISER_ADDR).await?;
        self.socket.send_to(&game_state.to_bytes(), RLVISER_ADDR).await?;

        Ok(())
    }

    pub async fn check_for_messages(&mut self) -> IoResult<Option<GameState>> {
        self.buffer.resize(GameState::MIN_NUM_BYTES, 0);
        let (num_bytes, _) = self.socket.peek_from(&mut self.buffer).await?;
        if num_bytes == 1 {
            // We got a connection and not a game state
            // So clear the byte from the socket buffer and return
            self.socket.recv_from(&mut self.buffer).await?;
            assert_eq!(self.buffer[0], 1);
            return Ok(None);
        }

        // the socket didn't send data back
        if self.buffer.is_empty() {
            return Ok(None);
        }

        // the socket sent data back
        // this is the other side telling us to update the game state
        let num_bytes = GameState::get_num_bytes(&self.buffer);
        self.buffer.resize(num_bytes, 0);
        self.socket.recv_from(&mut self.buffer).await?;

        Ok(Some(GameState::from_bytes(&self.buffer)))
    }

    pub async fn close(&mut self) -> IoResult<()> {
        self.socket.send_to(&[UdpPacketTypes::Quit as u8], RLVISER_ADDR).await?;
        println!("Sent quit signal to rlviser");

        Ok(())
    }
}
