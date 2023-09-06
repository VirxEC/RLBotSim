use std::{
    net::TcpStream,
    sync::RwLock,
    thread,
    time::Duration,
};

const MAX_CONNECTION_RETRIES: usize = 5;

static TCP_CONNECTION: RwLock<Option<TcpStream>> = RwLock::new(None);

#[no_mangle]
pub extern "C" fn IsInitialized() -> bool {
    true
}

#[no_mangle]
pub extern "C" fn StartTcpCommunication(
    port: i32,
    _wants_ball_predictions: bool,
    _wants_quick_chat: bool,
    _wants_game_messages: bool,
) -> i32 {
    thread::spawn(move || {
        let address = format!("127.0.0.1:{port}");

        for i in 1..=MAX_CONNECTION_RETRIES {
            let Ok(tcp_connection) = TcpStream::connect(&address) else {
                println!("Failed to connect to RLBot. Retry {i}/{MAX_CONNECTION_RETRIES}...");
                thread::sleep(Duration::from_secs(1));
                continue;
            };

            *TCP_CONNECTION.write().unwrap() = Some(tcp_connection);
            break;
        }
    });

    0
}

#[no_mangle]
pub extern "C" fn IsReadyForCommunication() -> bool {
    let Ok(lock) = TCP_CONNECTION.read() else {
        return false;
    };

    lock.is_some()
}
