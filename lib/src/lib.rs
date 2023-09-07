mod ctypes;
use ctypes::*;

use std::{
    io::{BufReader, BufWriter, Read, Write},
    net::TcpStream,
    sync::RwLock,
    thread,
    time::Duration,
};

use rlbot_core_types::{flatbuffers, gen::rlbot::flat, SocketDataType};

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

    dbg!(input, index);

    RLBotCoreStatus::Success as i32
}

static mut CURRENT_BYTE_BUFFER: Vec<u8> = Vec::new();

fn request_datatype(tcp: &mut TcpStream, request_data_type: SocketDataType) -> Vec<u8> {
    {
        let mut writer = BufWriter::new(tcp.try_clone().unwrap());
        writer.write_all(&(request_data_type as u16).to_be_bytes()).unwrap();
        writer.write_all(&1u16.to_be_bytes()).unwrap();
        writer.write_all(&[0]).unwrap();
        writer.flush().unwrap();
    }

    let mut reader = BufReader::new(tcp);

    loop {
        let mut msg = [0; 2];

        reader.read_exact(&mut msg).unwrap();
        let data_type = SocketDataType::from_u16(u16::from_be_bytes(msg));

        reader.read_exact(&mut msg).unwrap();
        let size = u16::from_be_bytes(msg) as usize;

        if data_type != request_data_type {
            reader.read_exact(&mut vec![0; size]).unwrap();
            continue;
        }

        let mut bytes = vec![0; size];
        reader.read_exact(&mut bytes).unwrap();
        break bytes;
    }
}

#[no_mangle]
pub extern "C" fn GetMatchSettings() -> ByteBuffer {
    let mut tcp_lock = TCP_CONNECTION.write().unwrap();
    let tcp = tcp_lock.as_mut().expect("TCP connection not initialized!");

    let bytes = request_datatype(tcp, SocketDataType::MatchSettings);

    unsafe {
        CURRENT_BYTE_BUFFER = bytes;

        ByteBuffer {
            data: CURRENT_BYTE_BUFFER.as_ptr(),
            size: CURRENT_BYTE_BUFFER.len(),
        }
    }
}

/// # Safety
/// It must be ensured that field_info is a valid pointer to a FieldInfoPacket.
#[no_mangle]
pub unsafe extern "C" fn UpdateFieldInfo(field_info: *mut FieldInfoPacket) -> i32 {
    let mut tcp_lock = TCP_CONNECTION.write().unwrap();
    let Some(tcp) = tcp_lock.as_mut() else {
        return RLBotCoreStatus::NotInitialized as i32;
    };

    let bytes = request_datatype(tcp, SocketDataType::FieldInfo);
    let packet = flatbuffers::root::<flat::FieldInfo<'_>>(&bytes).unwrap();

    let field_info = field_info.as_mut().unwrap();

    let field_pads = packet.boostPads().unwrap();
    field_info.num_boosts = field_pads.len() as i32;
    for (i, pad) in field_pads.iter().enumerate() {
        field_info.boost_pads[i].is_full_boost = pad.isFullBoost();
        let location = pad.location().unwrap();
        field_info.boost_pads[i].location.x = location.x();
        field_info.boost_pads[i].location.y = location.y();
        field_info.boost_pads[i].location.z = location.z();
    }

    let field_goals = packet.goals().unwrap();
    field_info.num_goals = field_goals.len() as i32;
    for (i, goal) in field_goals.iter().enumerate() {
        field_info.goals[i].team_num = goal.teamNum() as u8;
        let direction = goal.direction().unwrap();
        field_info.goals[i].direction.x = direction.x();
        field_info.goals[i].direction.y = direction.y();
        field_info.goals[i].direction.z = direction.z();
        let location = goal.location().unwrap();
        field_info.goals[i].location.x = location.x();
        field_info.goals[i].location.y = location.y();
        field_info.goals[i].location.z = location.z();
        field_info.goals[i].width = goal.width();
        field_info.goals[i].height = goal.height();
    }

    RLBotCoreStatus::Success as i32
}

/// # Safety
/// It must be ensured that there are no other references to the byte buffer.
#[no_mangle]
pub unsafe extern "C" fn Free(_ptr: u8) {
    CURRENT_BYTE_BUFFER = Vec::new();
}
