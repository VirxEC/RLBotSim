use crate::ctypes::{ByteBuffer, FieldInfoPacket, GameTickPacket, PlayerInput};
use rlbot_core_types::{flatbuffers, gen::rlbot::flat, SocketDataType};
use std::{
    io::{BufReader, BufWriter, Read, Write},
    net::TcpStream,
    time::Duration,
};

pub fn build_player_input(input: PlayerInput, index: i32) -> Vec<u8> {
    let mut builder = flatbuffers::FlatBufferBuilder::new();

    let controller_state_args = flat::ControllerStateArgs {
        throttle: input.throttle,
        steer: input.steer,
        pitch: input.pitch,
        yaw: input.yaw,
        roll: input.roll,
        jump: input.jump != 0,
        boost: input.boost != 0,
        handbrake: input.handbrake != 0,
        useItem: input.use_item != 0,
    };
    let controller_state = flat::ControllerState::create(&mut builder, &controller_state_args);

    let player_input_args = flat::PlayerInputArgs {
        playerIndex: index,
        controllerState: Some(controller_state),
    };
    let player_input = flat::PlayerInput::create(&mut builder, &player_input_args);

    builder.finish(player_input, None);
    builder.finished_data().to_vec()
}

/// "Properly" leaks a vector and stores it's pointer and length
/// MUST be manually freed or else there will be a memory leak
pub fn get_byte_buffer(bytes: Vec<u8>) -> ByteBuffer {
    let buffer: &'static mut [u8] = Box::leak(bytes.into_boxed_slice());

    ByteBuffer {
        data: buffer.as_mut_ptr(),
        size: buffer.len(),
    }
}

pub fn request_datatype(tcp: &mut TcpStream, request_data_type: SocketDataType, timeout_millis: Option<i32>) -> Vec<u8> {
    send_message(tcp.try_clone().unwrap(), request_data_type, &[0]);
    get_datatype(tcp, request_data_type, timeout_millis)
}

pub fn send_message(tcp: TcpStream, request_data_type: SocketDataType, message: &[u8]) {
    let mut writer = BufWriter::new(tcp);
    writer.write_all(&(request_data_type as u16).to_be_bytes()).unwrap();

    let message_num_bytes = u16::try_from(message.len()).unwrap();
    writer.write_all(&message_num_bytes.to_be_bytes()).unwrap();

    writer.write_all(message).unwrap();
    writer.flush().unwrap();
}

pub fn get_datatype(tcp: &mut TcpStream, request_data_type: SocketDataType, timeout_millis: Option<i32>) -> Vec<u8> {
    if let Some(millis) = timeout_millis {
        tcp.set_read_timeout(Some(Duration::from_millis(millis as u64))).unwrap();
    } else {
        tcp.set_read_timeout(None).unwrap();
    }

    let mut reader = BufReader::new(tcp);

    loop {
        let mut msg = [0; 2];

        if reader.read_exact(&mut msg).is_err() {
            break Vec::new();
        }

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

pub fn update_field_info(packet: flat::FieldInfo<'_>, field_info: &mut FieldInfoPacket) {
    let field_pads = packet.boostPads().unwrap();
    field_info.num_boosts = field_pads.len() as i32;
    for (flat_pad, field_pad) in field_pads.iter().zip(&mut field_info.boost_pads) {
        field_pad.is_full_boost = flat_pad.isFullBoost();

        let location = flat_pad.location().unwrap();
        field_pad.location.x = location.x();
        field_pad.location.y = location.y();
        field_pad.location.z = location.z();
    }

    let field_goals = packet.goals().unwrap();
    field_info.num_goals = field_goals.len() as i32;
    for (flat_goals, field_goal) in field_goals.iter().zip(&mut field_info.goals) {
        field_goal.team_num = flat_goals.teamNum() as u8;

        let direction = flat_goals.direction().unwrap();
        field_goal.direction.x = direction.x();
        field_goal.direction.y = direction.y();
        field_goal.direction.z = direction.z();

        let location = flat_goals.location().unwrap();
        field_goal.location.x = location.x();
        field_goal.location.y = location.y();
        field_goal.location.z = location.z();
        field_goal.width = flat_goals.width();
        field_goal.height = flat_goals.height();
    }
}

pub fn update_packet(packet: flat::GameTickPacket<'_>, game_tick_packet: &mut GameTickPacket) {
    let cars = packet.players().unwrap();
    game_tick_packet.num_cars = cars.len() as i32;
    for (flat_car, gtp_car) in cars.iter().zip(&mut game_tick_packet.game_cars) {
        let physics = flat_car.physics().unwrap();

        let location = physics.location().unwrap();
        gtp_car.physics.location.x = location.x();
        gtp_car.physics.location.y = location.y();
        gtp_car.physics.location.z = location.z();

        let rotation = physics.rotation().unwrap();
        gtp_car.physics.rotation.pitch = rotation.pitch();
        gtp_car.physics.rotation.yaw = rotation.yaw();
        gtp_car.physics.rotation.roll = rotation.roll();

        let velocity = physics.velocity().unwrap();
        gtp_car.physics.velocity.x = velocity.x();
        gtp_car.physics.velocity.y = velocity.y();
        gtp_car.physics.velocity.z = velocity.z();

        let angular_velocity = physics.angularVelocity().unwrap();
        gtp_car.physics.angular_velocity.x = angular_velocity.x();
        gtp_car.physics.angular_velocity.y = angular_velocity.y();
        gtp_car.physics.angular_velocity.z = angular_velocity.z();

        if let Some(score_info) = flat_car.scoreInfo() {
            // this isn't implemented yet
            gtp_car.score_info.score = score_info.score();
            gtp_car.score_info.goals = score_info.goals();
            gtp_car.score_info.own_goals = score_info.ownGoals();
            gtp_car.score_info.assists = score_info.assists();
            gtp_car.score_info.saves = score_info.saves();
            gtp_car.score_info.shots = score_info.shots();
            gtp_car.score_info.demolitions = score_info.demolitions();
        }

        gtp_car.is_demolished = flat_car.isDemolished();
        gtp_car.has_wheel_contact = flat_car.hasWheelContact();
        gtp_car.is_super_sonic = flat_car.isSupersonic();
        gtp_car.is_bot = flat_car.isBot();
        gtp_car.jumped = flat_car.jumped();
        gtp_car.double_jumped = flat_car.doubleJumped();

        for (flat_char, gtp_char) in flat_car.name().unwrap().chars().zip(&mut gtp_car.name) {
            *gtp_char = flat_char as u8;
        }

        gtp_car.team = flat_car.team() as u8;
        gtp_car.boost = flat_car.boost();

        let hitbox = flat_car.hitbox().unwrap();
        gtp_car.hitbox.length = hitbox.length();
        gtp_car.hitbox.width = hitbox.width();
        gtp_car.hitbox.height = hitbox.height();

        let hitbox_offset = flat_car.hitboxOffset().unwrap();
        gtp_car.hitbox_offset.x = hitbox_offset.x();
        gtp_car.hitbox_offset.y = hitbox_offset.y();
        gtp_car.hitbox_offset.z = hitbox_offset.z();

        gtp_car.spawn_id = flat_car.spawnId();
    }

    let boosts = packet.boostPadStates().unwrap();
    game_tick_packet.num_boost = boosts.len() as i32;
    for (flat_boost, gtp_boost) in boosts.iter().zip(&mut game_tick_packet.game_boosts) {
        gtp_boost.is_active = flat_boost.isActive();
        gtp_boost.timer = flat_boost.timer();
    }

    let ball = packet.ball().unwrap();
    let physics = ball.physics().unwrap();

    let location = physics.location().unwrap();
    game_tick_packet.game_ball.physics.location.x = location.x();
    game_tick_packet.game_ball.physics.location.y = location.y();
    game_tick_packet.game_ball.physics.location.z = location.z();

    let rotation = physics.rotation().unwrap();
    game_tick_packet.game_ball.physics.rotation.pitch = rotation.pitch();
    game_tick_packet.game_ball.physics.rotation.yaw = rotation.yaw();
    game_tick_packet.game_ball.physics.rotation.roll = rotation.roll();

    let velocity = physics.velocity().unwrap();
    game_tick_packet.game_ball.physics.velocity.x = velocity.x();
    game_tick_packet.game_ball.physics.velocity.y = velocity.y();
    game_tick_packet.game_ball.physics.velocity.z = velocity.z();

    let angular_velocity = physics.angularVelocity().unwrap();
    game_tick_packet.game_ball.physics.angular_velocity.x = angular_velocity.x();
    game_tick_packet.game_ball.physics.angular_velocity.y = angular_velocity.y();
    game_tick_packet.game_ball.physics.angular_velocity.z = angular_velocity.z();

    if let Some(latest_touch) = ball.latestTouch() {
        // this isn't implemented yet
        for (flat_char, gtp_char) in latest_touch
            .playerName()
            .unwrap()
            .chars()
            .zip(&mut game_tick_packet.game_ball.latest_touch.player_name)
        {
            *gtp_char = flat_char as u8;
        }

        game_tick_packet.game_ball.latest_touch.time_seconds = latest_touch.gameSeconds();

        let location = latest_touch.location().unwrap();
        game_tick_packet.game_ball.latest_touch.hit_location.x = location.x();
        game_tick_packet.game_ball.latest_touch.hit_location.y = location.y();
        game_tick_packet.game_ball.latest_touch.hit_location.z = location.z();

        let normal = latest_touch.normal().unwrap();
        game_tick_packet.game_ball.latest_touch.hit_normal.x = normal.x();
        game_tick_packet.game_ball.latest_touch.hit_normal.y = normal.y();
        game_tick_packet.game_ball.latest_touch.hit_normal.z = normal.z();

        game_tick_packet.game_ball.latest_touch.team = latest_touch.team();
        game_tick_packet.game_ball.latest_touch.player_index = latest_touch.playerIndex();
    }

    game_tick_packet.game_info.seconds_elapsed = packet.gameInfo().unwrap().secondsElapsed();
    game_tick_packet.game_info.game_time_remaining = packet.gameInfo().unwrap().gameTimeRemaining();
    game_tick_packet.game_info.is_overtime = packet.gameInfo().unwrap().isOvertime();
    game_tick_packet.game_info.is_unlimited_time = packet.gameInfo().unwrap().isUnlimitedTime();
    game_tick_packet.game_info.is_round_active = packet.gameInfo().unwrap().isRoundActive();
    game_tick_packet.game_info.is_kickoff_pause = packet.gameInfo().unwrap().isKickoffPause();
    game_tick_packet.game_info.is_match_ended = packet.gameInfo().unwrap().isMatchEnded();
    game_tick_packet.game_info.world_gravity_z = packet.gameInfo().unwrap().worldGravityZ();
    game_tick_packet.game_info.game_speed = packet.gameInfo().unwrap().gameSpeed();
    game_tick_packet.game_info.frame_num = packet.gameInfo().unwrap().frameNum();

    // dropshot_tiles is not implemented, skip it

    let teams = packet.teams().unwrap();
    game_tick_packet.num_teams = teams.len() as i32;
    for (flat_team, gtp_team) in teams.iter().zip(&mut game_tick_packet.teams) {
        gtp_team.team_index = flat_team.teamIndex();
        gtp_team.score = flat_team.score();
    }
}
