use rocketsim_rs::{
    bytes::{FromBytes, FromBytesExact, ToBytes},
    render::RenderMessage,
    GameState,
};
use std::{
    io::Result as IoResult,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::Path,
    process::Command,
};
use tokio::net::UdpSocket;

#[repr(u8)]
#[derive(Clone, Copy, Debug)]
enum UdpPacketTypes {
    Quit,
    GameState,
    Connection,
    Paused,
    Speed,
    Render,
}

impl From<u8> for UdpPacketTypes {
    fn from(val: u8) -> Self {
        match val {
            0 => Self::Quit,
            1 => Self::GameState,
            2 => Self::Connection,
            3 => Self::Paused,
            4 => Self::Speed,
            5 => Self::Render,
            _ => panic!("Invalid packet type"),
        }
    }
}

pub enum StateControl {
    None,
    GameState(GameState),
    Speed(f32),
    Paused(bool),
}

pub struct ExternalManager {
    rlviser_addr: SocketAddr,
    socket: UdpSocket,
    buffer: Vec<u8>,
}

impl ExternalManager {
    pub async fn new(
        rlviser_path: &Path,
        rlviser_port: u16,
        rocketsim_port: u16,
    ) -> IoResult<Self> {
        Command::new(rlviser_path.canonicalize()?)
            .args([rocketsim_port.to_string(), rlviser_port.to_string()])
            .env("CARGO_MANIFEST_DIR", "")
            .spawn()?;

        Ok(Self {
            rlviser_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), rlviser_port),
            socket: UdpSocket::bind((Ipv4Addr::new(0, 0, 0, 0), rocketsim_port)).await?,
            buffer: Vec::with_capacity(1024),
        })
    }

    pub async fn send_render_group(&self, group: RenderMessage) -> IoResult<()> {
        self.socket
            .send_to(&[UdpPacketTypes::Render as u8], self.rlviser_addr)
            .await?;
        self.socket
            .send_to(&group.to_bytes(), self.rlviser_addr)
            .await?;

        Ok(())
    }

    pub async fn send_game_state(&self, game_state: &GameState) -> IoResult<()> {
        self.socket
            .send_to(&[UdpPacketTypes::GameState as u8], self.rlviser_addr)
            .await?;
        self.socket
            .send_to(&game_state.to_bytes(), self.rlviser_addr)
            .await?;

        Ok(())
    }

    pub async fn check_for_messages(&mut self) -> IoResult<StateControl> {
        self.buffer.resize(1, 0);
        let (_, src) = self.socket.recv_from(&mut self.buffer).await?;
        let packet_type = UdpPacketTypes::from(self.buffer[0]);

        match packet_type {
            UdpPacketTypes::GameState => {
                self.buffer.resize(GameState::MIN_NUM_BYTES, 0);
                self.socket.peek_from(&mut self.buffer).await?;

                let num_bytes = GameState::get_num_bytes(&self.buffer);
                self.buffer.resize(num_bytes, 0);
                self.socket.recv_from(&mut self.buffer).await?;

                Ok(StateControl::GameState(GameState::from_bytes(&self.buffer)))
            }
            UdpPacketTypes::Speed => {
                self.buffer.resize(f32::NUM_BYTES, 0);
                self.socket.recv_from(&mut self.buffer).await?;

                Ok(StateControl::Speed(f32::from_bytes(&self.buffer)))
            }
            UdpPacketTypes::Paused => {
                // the buffer is already the correct size (1)
                self.socket.recv_from(&mut self.buffer).await?;

                Ok(StateControl::Paused(self.buffer[0] == 1))
            }
            UdpPacketTypes::Connection => {
                println!("Connection established to {src}");
                Ok(StateControl::None)
            }
            UdpPacketTypes::Quit | UdpPacketTypes::Render => {
                println!("We shouldn't be receiving packets of type {packet_type:?}");
                Ok(StateControl::None)
            }
        }
    }

    pub async fn close(&self) -> IoResult<()> {
        self.socket
            .send_to(&[UdpPacketTypes::Quit as u8], self.rlviser_addr)
            .await?;
        println!("Sent quit signal to rlviser");

        Ok(())
    }
}
