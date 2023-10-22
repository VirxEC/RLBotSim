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

    // this pointer to reference is unsafe because we don't know if the pointer is valid
    let gtp = game_tick_packet.as_mut().unwrap();

    update_packet(packet, gtp);

    RLBotCoreStatus::Success as i32
}

/// # Safety
/// It must be ensured that there are no other references this item when calling this function
#[no_mangle]
pub unsafe extern "C" fn Free(ptr: *mut u8) {
    dealloc(ptr, Layout::new::<&'static mut [u8]>());
}

// func = self.game.SendQuickChat
// func.argtypes = [ctypes.c_void_p, ctypes.c_int]
// func.restype = ctypes.c_int
#[no_mangle]
pub extern "C" fn SendQuickChat(_ptr: usize, _quick_chat: i32) -> i32 {
    RLBotCoreStatus::Success as i32
}

// Renderer stuff
// Might never implement lol

// self.native_constructor = dll_instance.Renderer_Constructor
// self.native_constructor.argtypes = [ctypes.c_int]
// self.native_constructor.restype = ctypes.c_void_p
#[no_mangle]
pub extern "C" fn Renderer_Constructor(_group_id_hashed: i32) -> usize {
    1
}

// self.native_destructor = dll_instance.Renderer_Destructor
// self.native_destructor.argtypes = [ctypes.c_void_p]
#[no_mangle]
pub extern "C" fn Renderer_Destructor(_ptr: usize) {}

// self.native_finish_and_send = dll_instance.Renderer_FinishAndSend
// self.native_finish_and_send.argtypes = [ctypes.c_void_p]
#[no_mangle]
pub extern "C" fn Renderer_FinishAndSend(_ptr: usize) {}

// self.native_draw_line_3d = dll_instance.Renderer_DrawLine3D
// self.native_draw_line_3d.argtypes = [ctypes.c_void_p, Color, Vector3, Vector3]
#[no_mangle]
pub extern "C" fn Renderer_DrawLine3D(_ptr: usize, _color: Color, _start: Vector3, _end: Vector3) {}

// self.native_draw_polyline_3d = dll_instance.Renderer_DrawPolyLine3D
// self.native_draw_polyline_3d.argtypes = [ctypes.c_void_p, Color, ctypes.POINTER(Vector3), ctypes.c_int]
#[no_mangle]
pub extern "C" fn Renderer_DrawPolyLine3D(_ptr: usize, _color: Color, _points: *const Vector3, _num_points: i32) {}

// self.native_draw_string_2d = dll_instance.Renderer_DrawString2D
// self.native_draw_string_2d.argtypes = [ctypes.c_void_p, ctypes.c_char_p, Color, Vector3, ctypes.c_int, ctypes.c_int]
#[no_mangle]
pub extern "C" fn Renderer_DrawString2D(
    _ptr: usize,
    _text: *const u8,
    _color: Color,
    _upper_left: Vector3,
    _scale_x: i32,
    _scale_y: i32,
) {
}

// self.native_draw_string_3d = dll_instance.Renderer_DrawString3D
// self.native_draw_string_3d.argtypes = [ctypes.c_void_p, ctypes.c_char_p, Color, Vector3, ctypes.c_int, ctypes.c_int]
#[no_mangle]
pub extern "C" fn Renderer_DrawString3D(
    _ptr: usize,
    _text: *const u8,
    _color: Color,
    _upper_left: Vector3,
    _scale_x: i32,
    _scale_y: i32,
) {
}

// self.native_draw_rect_2d = dll_instance.Renderer_DrawRect2D
// self.native_draw_rect_2d.argtypes = [ctypes.c_void_p, Color, Vector3, ctypes.c_int, ctypes.c_int, ctypes.c_bool]
#[no_mangle]
pub extern "C" fn Renderer_DrawRect2D(
    _ptr: usize,
    _color: Color,
    _upper_left: Vector3,
    _width: i32,
    _height: i32,
    _filled: bool,
) {
}

// self.native_draw_rect_3d = dll_instance.Renderer_DrawRect3D
// self.native_draw_rect_3d.argtypes = [ctypes.c_void_p, Color, Vector3, ctypes.c_int, ctypes.c_int, ctypes.c_bool, ctypes.c_bool]
#[no_mangle]
pub extern "C" fn Renderer_DrawRect3D(
    _ptr: usize,
    _color: Color,
    _upper_left: Vector3,
    _width: i32,
    _height: i32,
    _filled: bool,
    _occluded: bool,
) {
}
