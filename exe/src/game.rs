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
    state_type: flat::GameStateType,
    match_settings: Option<(flat::MatchSettingsT, Box<[u8]>)>,
    field_info: Option<Box<[u8]>>,
}

impl<'a> Game<'a> {
    fn new(tx: broadcast::Sender<messages::FromGame>) -> Self {
        Self {
            tx,
            arena: Arena::default_standard(),
            flat_builder: FlatBufferBuilder::with_capacity(10240),
            state_type: flat::GameStateType::Inactive,
            match_settings: None,
            field_info: None,
        }
    }

    fn handle_message_from_client(&mut self, msg: messages::ToGame) -> bool {
        match msg {
            messages::ToGame::FieldInfoRequest(sender) => {
                if let Some(field_info) = &self.field_info {
                    sender.send(field_info.clone()).unwrap();
                }
            }
            messages::ToGame::MatchSettingsRequest(sender) => {
                if let Some((_, bytes)) = &self.match_settings {
                    sender.send(bytes.clone()).unwrap();
                }
            }
            messages::ToGame::MatchSettings(match_settings) => {
                self.state_type = flat::GameStateType::Active;

                util::auto_start_bots(&match_settings).unwrap();
                self.set_match_settings(match_settings);
                self.set_field_info();
            }
            messages::ToGame::StopCommand(info) => {
                self.state_type = flat::GameStateType::Ended;

                self.tx.send(messages::FromGame::None).unwrap();

                if info.shutdown_server {
                    return false;
                }
            }
        }

        true
    }

    fn set_match_settings(&mut self, match_settings: flat::MatchSettingsT) {
        self.flat_builder.reset();
        let offset = match_settings.pack(&mut self.flat_builder);
        self.flat_builder.finish(offset, None);
        let bytes = self.flat_builder.finished_data();

        self.match_settings = Some((match_settings, bytes.into()));
    }

    fn set_field_info(&mut self) {
        let mut field_info = flat::FieldInfoT::default();

        let mut blue_goal = flat::GoalInfoT::default();
        blue_goal.team_num = 0;
        blue_goal.location = flat::Vector3T {
            x: 0.,
            y: -5120.,
            z: 642.775,
        };
        blue_goal.direction = flat::Vector3T { x: 0., y: 1., z: 0. };
        blue_goal.width = 892.755;
        blue_goal.height = 642.775;

        let mut orange_goal = flat::GoalInfoT::default();
        orange_goal.team_num = 1;
        orange_goal.location = flat::Vector3T {
            x: 0.,
            y: 5120.,
            z: 642.775,
        };
        orange_goal.direction = flat::Vector3T { x: 0., y: -1., z: 0. };
        orange_goal.width = 892.755;
        orange_goal.height = 642.775;

        field_info.goals.reserve(2);
        field_info.goals.push(blue_goal);
        field_info.goals.push(orange_goal);

        field_info
            .boost_pads
            .extend(self.arena.iter_pad_static().map(|(is_full_boost, location)| {
                let mut boost_pad = flat::BoostPadT::default();

                boost_pad.is_full_boost = is_full_boost;
                boost_pad.location = flat::Vector3T {
                    x: location.x,
                    y: location.y,
                    z: location.z,
                };
                boost_pad
            }));

        self.flat_builder.reset();
        let offset = field_info.pack(&mut self.flat_builder);
        self.flat_builder.finish(offset, None);
        let bytes = self.flat_builder.finished_data();

        self.field_info = Some(bytes.into());
    }

    fn advance_game(&mut self) {
        if self.state_type == flat::GameStateType::Active || self.state_type == flat::GameStateType::Kickoff {
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
        packet.game_info.game_state_type = self.state_type;

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
