use rlbot_sockets::{flat, flatbuffers::FlatBufferBuilder, SocketDataType};
use std::{io::Result as IoResult, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    time::sleep,
};

struct Connection<'a> {
    tcp: TcpStream,
    builder: FlatBufferBuilder<'a>,
    buffer: Vec<u8>,
}

impl<'a> Connection<'a> {
    async fn new() -> IoResult<Self> {
        Ok(Self {
            tcp: TcpStream::connect("127.0.0.1:23234").await?,
            builder: FlatBufferBuilder::with_capacity(1024),
            buffer: Vec::with_capacity(1024),
        })
    }

    async fn send_flatbuffer(&mut self, data_type: SocketDataType) -> IoResult<()> {
        let flat = self.builder.finished_data();

        self.buffer.clear();
        self.buffer.reserve(4 + flat.len());

        self.buffer
            .extend_from_slice(&(data_type as u16).to_be_bytes());
        let size = u16::try_from(flat.len()).expect("Flatbuffer too large");
        self.buffer.extend_from_slice(&size.to_be_bytes());
        self.buffer.extend_from_slice(flat);

        self.tcp.write_all(&self.buffer).await?;
        self.tcp.flush().await?;
        Ok(())
    }

    async fn read_flatbuffer(&mut self) -> IoResult<SocketDataType> {
        let data_type = self.tcp.read_u16().await?;
        let size = self.tcp.read_u16().await?;

        self.buffer.resize(usize::from(size), 0);
        self.tcp.read_exact(&mut self.buffer).await?;

        Ok(SocketDataType::from_u16(data_type))
    }

    async fn wait_for_type(&mut self, data_type: SocketDataType) -> IoResult<()> {
        loop {
            let received_type = self.read_flatbuffer().await?;
            if received_type == data_type {
                break;
            }
        }

        Ok(())
    }

    async fn connect(&mut self) -> IoResult<()> {
        let mut ready_message = flat::ConnectionSettingsT::default();
        ready_message.wants_ball_predictions = true;

        self.builder.reset();
        let offset = ready_message.pack(&mut self.builder);
        self.builder.finish(offset, None);

        self.send_flatbuffer(SocketDataType::ConnectionSettings)
            .await?;

        Ok(())
    }

    async fn start_match(&mut self) -> IoResult<()> {
        let match_settings = "./exe/examples/run_match.toml";
        let mut start = flat::StartCommandT::default();
        start.config_path = match_settings.to_string();

        self.builder.reset();
        let offset = start.pack(&mut self.builder);
        self.builder.finish(offset, None);

        self.send_flatbuffer(SocketDataType::StartCommand).await?;

        self.wait_for_type(SocketDataType::GamePacket).await?;
        self.wait_for_type(SocketDataType::BallPrediction).await?;

        Ok(())
    }

    async fn stop_match(&mut self) -> IoResult<()> {
        let mut stop_message = flat::StopCommandT::default();
        stop_message.shutdown_server = true;

        self.builder.reset();
        let offset = stop_message.pack(&mut self.builder);
        self.builder.finish(offset, None);

        self.send_flatbuffer(SocketDataType::StopCommand).await?;

        self.wait_for_type(SocketDataType::None).await?;

        Ok(())
    }
}

#[tokio::main]
async fn main() -> IoResult<()> {
    let mut connection = Connection::new().await?;

    connection.connect().await?;
    connection.start_match().await?;

    sleep(Duration::from_secs(300)).await;

    connection.stop_match().await?;

    Ok(())
}
