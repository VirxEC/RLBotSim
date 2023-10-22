mod ctypes;
mod util;

use ctypes::*;
use rlbot_core_types::{flatbuffers, gen::rlbot::flat, SocketDataType};
use std::{
    alloc::{dealloc, Layout},
    net::TcpStream,
    sync::RwLock,
    thread,
    time::Duration,
};
use util::*;

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
        dbg!(&address);

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
    TCP_CONNECTION.read().is_ok_and(|tcp| tcp.is_some())
}

#[no_mangle]
pub extern "C" fn UpdatePlayerInput(input: PlayerInput, index: i32) -> i32 {
    let mut tcp_lock = TCP_CONNECTION.write().unwrap();
    let Some(tcp) = tcp_lock.as_mut() else {
        return RLBotCoreStatus::NotInitialized as i32;
    };

    let message = build_player_input(input, index);
    send_message(tcp.try_clone().unwrap(), SocketDataType::PlayerInput, &message);

    RLBotCoreStatus::Success as i32
}

#[no_mangle]
pub extern "C" fn GetMatchSettings() -> ByteBuffer {
    let mut tcp_lock = TCP_CONNECTION.write().unwrap();
    let tcp = tcp_lock.as_mut().expect("TCP connection not initialized!");

    let bytes = request_datatype(tcp, SocketDataType::MatchSettings, None);
    get_byte_buffer(bytes)
}

#[no_mangle]
pub extern "C" fn ReceiveChat(_index: i32, _team: i32, _last_message_index: i32) -> ByteBuffer {
    // TODO: Actually implement QuickChats

    let mut builder = flatbuffers::FlatBufferBuilder::new();
    let messages_args = flat::QuickChatMessagesArgs {
        messages: Some(builder.create_vector::<flatbuffers::ForwardsUOffset<flat::QuickChat>>(&[])),
    };
    let messages = flat::QuickChatMessages::create(&mut builder, &messages_args);
    builder.finish(messages, None);
    let data = builder.finished_data();

    let mut bytes = Vec::with_capacity(data.len());
    bytes.extend_from_slice(data);

    get_byte_buffer(bytes)
}

/// # Safety
/// It must be ensured that field_info is a valid pointer to a FieldInfoPacket.
#[no_mangle]
pub unsafe extern "C" fn UpdateFieldInfo(field_info: *mut FieldInfoPacket) -> i32 {
    let mut tcp_lock = TCP_CONNECTION.write().unwrap();
    let Some(tcp) = tcp_lock.as_mut() else {
        return RLBotCoreStatus::NotInitialized as i32;
    };

    let bytes = request_datatype(tcp, SocketDataType::FieldInfo, None);
    let packet = flatbuffers::root::<flat::FieldInfo<'_>>(&bytes).unwrap();

    let field_info = field_info.as_mut().unwrap();

    update_field_info(packet, field_info);

    RLBotCoreStatus::Success as i32
}

/// # Safety
/// It must be ensured that game_tick_packet is a valid pointer to a GameTickPacket.
#[no_mangle]
pub unsafe extern "C" fn FreshLiveDataPacket(game_tick_packet: *mut GameTickPacket, timeout_millis: i32, _key: i32) -> i32 {
    let mut tcp_lock = TCP_CONNECTION.write().unwrap();
    let Some(tcp) = tcp_lock.as_mut() else {
        return RLBotCoreStatus::NotInitialized as i32;
    };

    let bytes = get_datatype(tcp, SocketDataType::GameTickPacket, Some(timeout_millis));

    if bytes.is_empty() {
        // temporary return value
        return RLBotCoreStatus::MessageLargerThanMax as i32;
    }

    let packet = flatbuffers::root::<flat::GameTickPacket<'_>>(&bytes).unwrap();

    let game_tick_packet = game_tick_packet.as_mut().unwrap();

    update_packet(packet, game_tick_packet);

    RLBotCoreStatus::Success as i32
}

/// # Safety
/// It must be ensured that there are no other references this item when calling this function
#[no_mangle]
pub unsafe extern "C" fn Free(ptr: *mut u8) {
    dealloc(ptr, Layout::new::<&'static mut [u8]>());
}

// Renderer stuff
// Might never implement lol

#[no_mangle]
pub extern "C" fn Renderer_Constructor(_group_id_hashed: i32) -> usize {
    0
}

#[no_mangle]
pub extern "C" fn Renderer_Destructor(_ptr: usize) {}

#[no_mangle]
pub extern "C" fn Renderer_FinishAndSend(_ptr: usize) {}
