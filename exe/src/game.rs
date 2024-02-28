use rlbot_sockets::{
    flat::{self, CollisionShapeT},
    flatbuffers::FlatBufferBuilder,
};
use rocketsim_rs::{bytes::ToBytes, cxx::UniquePtr, init, sim::Arena, GameState};
use std::time::Duration;
use tokio::{
    sync::{broadcast, mpsc},
    time::interval,
};

use crate::{messages, util};

const GAME_TPS: f32 = 1. / 120.;

struct Game<'a> {
    arena: UniquePtr<Arena>,
    tx: broadcast::Sender<messages::FromGame>,
    flat_builder: FlatBufferBuilder<'a>,
    game_state_type: flat::GameStateType,
    match_settings: Option<(flat::MatchSettingsT, Box<[u8]>)>,
}

impl<'a> Game<'a> {
    fn new(tx: broadcast::Sender<messages::FromGame>) -> Self {
        Self {
            tx,
            arena: Arena::default_standard(),
            flat_builder: FlatBufferBuilder::with_capacity(10240),
            game_state_type: flat::GameStateType::Inactive,
            match_settings: None,
        }
    }

    fn handle_message_from_client(&mut self, msg: messages::ToGame) -> bool {
        match msg {
            messages::ToGame::MatchSettingsRequest(sender) => {
                if let Some((_, bytes)) = &self.match_settings {
                    sender.send(bytes.clone()).unwrap();
                }
            }
            messages::ToGame::MatchSettings(match_settings) => {
                self.game_state_type = flat::GameStateType::Active;

                util::auto_start_bots(&match_settings).unwrap();

                self.flat_builder.reset();
                let offset = match_settings.pack(&mut self.flat_builder);
                self.flat_builder.finish(offset, None);
                let bytes = self.flat_builder.finished_data();

                self.match_settings = Some((match_settings, bytes.into()));
            }
            messages::ToGame::StopCommand(info) => {
                self.game_state_type = flat::GameStateType::Ended;

                self.tx.send(messages::FromGame::None).unwrap();

                if info.shutdown_server {
                    return false;
                }
            }
        }

        true
    }

    fn advance_game(&mut self) {
        if self.game_state_type == flat::GameStateType::Active || self.game_state_type == flat::GameStateType::Kickoff {
            self.arena.pin_mut().step(1);
        }

        let game_state = self.arena.pin_mut().get_game_state();
        let _bytes = game_state.to_bytes();
        // TODO: RLViser stuff

        // construct and send out game tick packet
        let packet = self.make_game_tick_packet(&game_state);

        self.flat_builder.reset();
        let offset = packet.pack(&mut self.flat_builder);
        self.flat_builder.finish(offset, None);
        let bytes = self.flat_builder.finished_data();

        let _ = self.tx.send(messages::FromGame::GameTickPacket(bytes.into()));
    }

    fn make_game_tick_packet(&self, game_state: &GameState) -> flat::GameTickPacketT {
        let mut packet = flat::GameTickPacketT::default();

        let mut sphere_shape: Box<flat::SphereShapeT> = Box::default();
        sphere_shape.diameter = 91.25 * 2.;
        packet.ball.shape = CollisionShapeT::SphereShape(sphere_shape);

        packet.game_info.seconds_elapsed = game_state.tick_count as f32 * GAME_TPS;
        packet.game_info.frame_num = game_state.tick_count as u32;
        packet.game_info.game_state_type = self.game_state_type;

        packet
    }
}

#[tokio::main]
pub async fn run_rl(
    tx: broadcast::Sender<messages::FromGame>,
    mut rx: mpsc::Receiver<messages::ToGame>,
    shutdown_sender: mpsc::Sender<()>,
) {
    init(None);

    let mut interval = interval(Duration::from_secs_f32(GAME_TPS));
    let mut game = Game::new(tx);

    loop {
        tokio::select! {
            Some(msg) = rx.recv() => {
                if !game.handle_message_from_client(msg) {
                    break;
                }
            }
            // make tokio timer that goes off 120 times per second
            // every time it goes off, send a game tick packet to the client
            _ = interval.tick() => game.advance_game(),
            else => break,
        }
    }

    println!("Shutting down RocketSim");
    shutdown_sender.send(()).await.unwrap();
}
