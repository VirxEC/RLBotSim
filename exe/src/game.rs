use rlbot_core_types::{
    flat::{self, CollisionShapeT},
    flatbuffers, RustMessage,
};
use rocketsim_rs::{init, sim::Arena};
use std::{env::current_dir, pin::Pin, time::Duration};
use tokio::{
    sync::{broadcast, mpsc},
    time::interval,
};

const GAME_TPS: f32 = 1. / 120.;

#[tokio::main]
pub async fn run_rl(
    tx: broadcast::Sender<RustMessage>,
    mut rx: mpsc::Receiver<RustMessage>,
    shutdown_sender: mpsc::Sender<()>,
) {
    let cwd = current_dir().unwrap().join("collision_meshes");
    init(Some(&cwd.display().to_string()));

    let mut flat_builder = flatbuffers::FlatBufferBuilder::with_capacity(10240);

    let mut game = Arena::default_standard();
    let mut interval = interval(Duration::from_secs_f32(GAME_TPS));

    let mut game_state = flat::GameStateType::Inactive;

    loop {
        tokio::select! {
            Some(msg) = rx.recv() => {
                if !handle_message_from_client(&tx, msg, game.pin_mut(), &mut game_state) {
                    break;
                }
            }
            // make tokio timer that goes off 120 times per second
            // every time it goes off, send a game tick packet to the client
            _ = interval.tick() => {
                advance_game(game_state, game.pin_mut(), &tx, &mut flat_builder)
            }
            else => break,
        }
    }

    println!("Shutting down RocketSim");
    shutdown_sender.send(()).await.unwrap();
}

fn handle_message_from_client(
    tx: &broadcast::Sender<RustMessage>,
    msg: RustMessage,
    game: Pin<&mut Arena>,
    game_state: &mut flat::GameStateType,
) -> bool {
    match msg {
        RustMessage::MatchSettings(match_settings) => {
            dbg!(match_settings.player_configurations.len());
            *game_state = flat::GameStateType::Active;
        }
        RustMessage::PlayerInput(input) => {
            dbg!(input);
        }
        RustMessage::StopCommand(info) => {
            *game_state = flat::GameStateType::Ended;

            tx.send(RustMessage::None).unwrap();

            if info.shutdown_server {
                return false;
            }
        }
        _ => {}
    }

    true
}

fn advance_game(
    game_state: flat::GameStateType,
    mut game: Pin<&mut Arena>,
    tx: &broadcast::Sender<RustMessage>,
    flat_builder: &mut flatbuffers::FlatBufferBuilder,
) {
    if game_state == flat::GameStateType::Active || game_state == flat::GameStateType::Kickoff {
        game.as_mut().step(1);
    }

    // construct and send out game tick packet
    let packet = make_game_tick_packet(game, game_state);

    flat_builder.reset();
    let offset = packet.pack(flat_builder);
    flat_builder.finish(offset, None);
    let bytes = flat_builder.finished_data();

    let _ = tx.send(RustMessage::GameTickPacket(bytes.into()));
}

fn make_game_tick_packet(game: Pin<&mut Arena>, game_state_type: flat::GameStateType) -> flat::GameTickPacketT {
    let mut packet = flat::GameTickPacketT::default();
    let game_state = game.get_game_state();

    let mut sphere_shape: Box<flat::SphereShapeT> = Box::default();
    sphere_shape.diameter = 91.25 * 2.;
    packet.ball.shape = CollisionShapeT::SphereShape(sphere_shape);

    packet.game_info.seconds_elapsed = game_state.tick_count as f32 * GAME_TPS;
    packet.game_info.frame_num = game_state.tick_count as u32;
    packet.game_info.game_state_type = game_state_type;

    packet
}
