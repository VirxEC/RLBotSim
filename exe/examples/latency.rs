use rlbot_sockets::{flat, flatbuffers::FlatBufferBuilder, SocketDataType};
use std::{
    io::{Result as IoResult, Write},
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
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

        self.buffer.extend_from_slice(&(data_type as u16).to_be_bytes());
        assert!(flat.len() <= u16::MAX as usize, "Flatbuffer too large");
        self.buffer.extend_from_slice(&(flat.len() as u16).to_be_bytes());
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
        let ready_message = flat::ReadyMessageT::default();

        self.builder.reset();
        let offset = ready_message.pack(&mut self.builder);
        self.builder.finish(offset, None);

        self.send_flatbuffer(SocketDataType::ReadyMessage).await?;

        // wait for a message back
        // we don't care about the message, just that we got one
        self.read_flatbuffer().await?;

        Ok(())
    }

    async fn start_match(&mut self) -> IoResult<()> {
        let match_settings = "./exe/examples/latency.toml";
        let mut start = flat::StartCommandT::default();
        start.config_path = match_settings.to_string();

        self.builder.reset();
        let offset = start.pack(&mut self.builder);
        self.builder.finish(offset, None);

        self.send_flatbuffer(SocketDataType::StartCommand).await?;

        self.wait_for_type(SocketDataType::GameTickPacket).await?;

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

    // measure stuff

    let start_time = Instant::now();
    connection.connect().await?;
    let end_time = Instant::now();
    println!("Time to connect: {:?}", end_time - start_time);

    let start_time = Instant::now();
    connection.start_match().await?;
    let end_time = Instant::now();
    println!("Time to start match: {:?}", end_time - start_time);

    println!("Measuring time to receive GameTickPackets...");

    let mut times = Vec::new();
    let mut num_spkies = 0;
    for i in 0..6000 {
        let start_time = Instant::now();
        connection.wait_for_type(SocketDataType::GameTickPacket).await?;
        let end_time = Instant::now();

        let diff = end_time - start_time;
        times.push(diff);

        if diff > Duration::from_millis(9) {
            num_spkies += 1;
            print!("Spikes: {}%\r", num_spkies as f32 / i as f32 * 100.);
            std::io::stdout().flush().unwrap();
        }
    }

    let sum: u128 = times.iter().map(|t| t.as_nanos()).sum();
    let average = sum / times.len() as u128;
    let average_ms = average as f64 / 1_000_000.0;
    println!("Average time to receive GameTickPacket: {average_ms:?}ms");

    // calculate .1% and 99.9% percentiles
    times.sort();
    let percentile_001 = times[times.len() / 1000];
    let percentile_999 = times[times.len() - times.len() / 1000 - 1];
    println!("0.1 percentile: {percentile_001:?}");
    println!("99.9 percentile: {percentile_999:?}");

    let start_time = Instant::now();
    connection.stop_match().await?;
    let end_time = Instant::now();
    println!("Time to stop match: {:?}", end_time - start_time);

    Ok(())
}
