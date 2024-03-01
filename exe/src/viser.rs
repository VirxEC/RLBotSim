use rocketsim_rs::{
    bytes::{FromBytes, ToBytes},
    GameState,
};
use std::{io::Result as IoResult, net::SocketAddr, process::Command};
use tokio::net::UdpSocket;

const RLVISER_PATH: &str = if cfg!(windows) { "./rlviser.exe" } else { "./rlviser" };

#[repr(u8)]
pub enum UdpPacketTypes {
    Quit,
    GameState,
}

pub struct ExternalManager {
    socket: UdpSocket,
    buffer: Vec<u8>,
    src: SocketAddr,
}

impl ExternalManager {
    pub async fn new() -> IoResult<Self> {
        let socket = UdpSocket::bind("0.0.0.0:34254").await?;
        Command::new(RLVISER_PATH).spawn()?;

        let mut buffer = Vec::with_capacity(1024);
        buffer.resize(1, 0);

        let (_, src) = socket.recv_from(&mut buffer).await?;
        assert_eq!(buffer[0], 1);

        Ok(Self { socket, buffer, src })
    }

    pub async fn send_game_state(&mut self, game_state: &GameState) -> IoResult<()> {
        self.socket.send_to(&[UdpPacketTypes::GameState as u8], self.src).await?;
        self.socket.send_to(&game_state.to_bytes(), self.src).await?;

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
        self.socket.send_to(&[UdpPacketTypes::Quit as u8], self.src).await?;
        println!("Sent quit signal to rlviser");

        Ok(())
    }
}
