use rlbot_sockets::{flat, flatbuffers::FlatBufferBuilder};
use rocketsim_rs::{
    cxx::UniquePtr,
    init,
    math::Angle,
    sim::{Arena, CarConfig, CarControls, Team},
    GameState,
};
use std::{
    collections::HashMap,
    io::Result as IoResult,
    sync::atomic::{AtomicBool, AtomicU32, Ordering},
    time::Duration,
};
use tokio::{
    sync::{broadcast, mpsc},
    time::{interval, Interval},
};

use crate::{messages, util, viser};

const GAME_TPS: u8 = 120;
const GAME_DT: f32 = 1. / GAME_TPS as f32;

static BLUE_SCORE: AtomicU32 = AtomicU32::new(0);
static ORANGE_SCORE: AtomicU32 = AtomicU32::new(0);
static NEEDS_RESET: AtomicBool = AtomicBool::new(false);

struct Game<'a> {
    arena: UniquePtr<Arena>,
    tx: broadcast::Sender<messages::FromGame>,
    flat_builder: FlatBufferBuilder<'a>,
    state_type: flat::GameStateType,
    countdown_end_tick: u64,
    match_settings: Option<(flat::MatchSettingsT, Box<[u8]>)>,
    field_info: Option<Box<[u8]>>,
    extra_car_info: HashMap<usize, (String, u32, i32), ahash::RandomState>,
}

impl<'a> Game<'a> {
    fn new(tx: broadcast::Sender<messages::FromGame>) -> Self {
        Self {
            tx,
            arena: Arena::default_standard(),
            flat_builder: FlatBufferBuilder::with_capacity(10240),
            state_type: flat::GameStateType::Inactive,
            countdown_end_tick: 0,
            match_settings: None,
            field_info: None,
            extra_car_info: HashMap::default(),
        }
    }

    fn set_state_to_countdown(&mut self) {
        self.state_type = flat::GameStateType::Countdown;
        self.countdown_end_tick = self.arena.get_tick_count() + 3 * GAME_TPS as u64;
    }

    fn handle_message_from_client(&mut self, msg: messages::ToGame) -> IoResult<bool> {
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
                self.set_state_to_countdown();
                util::auto_start_bots(&match_settings)?;
                self.set_match_settings(match_settings);
                self.set_field_info();
            }
            messages::ToGame::PlayerInput(input) => {
                let car_id = self.extra_car_info[&(input.player_index as usize)].1;
                let car_controls = CarControls {
                    throttle: input.controller_state.throttle,
                    steer: input.controller_state.steer,
                    pitch: input.controller_state.pitch,
                    yaw: input.controller_state.yaw,
                    roll: input.controller_state.roll,
                    boost: input.controller_state.boost,
                    jump: input.controller_state.jump,
                    handbrake: input.controller_state.handbrake,
                };

                self.arena.pin_mut().set_car_controls(car_id, car_controls).unwrap();
            }
            messages::ToGame::StopCommand(info) => {
                self.state_type = flat::GameStateType::Ended;

                self.tx.send(messages::FromGame::None).unwrap();

                if info.shutdown_server {
                    return Ok(false);
                }
            }
        }

        Ok(true)
    }

    fn set_state(&mut self, game_state: &GameState) {
        self.arena.pin_mut().set_game_state(game_state).unwrap();
    }

    fn set_match_settings(&mut self, match_settings: flat::MatchSettingsT) {
        self.arena = match match_settings.game_mode {
            flat::GameMode::Soccer => Arena::default_standard(),
            flat::GameMode::Hoops => Arena::default_hoops(),
            flat::GameMode::Heatseeker => Arena::default_heatseeker(),
            // flat::GameMode::Hockey => Arena::default_snowday(),
            _ => unimplemented!(),
        };

        self.arena.pin_mut().set_goal_scored_callback(
            |arena, car_team, _| {
                NEEDS_RESET.store(true, Ordering::Relaxed);

                match car_team {
                    Team::BLUE => {
                        BLUE_SCORE.fetch_add(1, Ordering::Relaxed);
                    }
                    Team::ORANGE => {
                        ORANGE_SCORE.fetch_add(1, Ordering::Relaxed);
                    }
                }

                arena.reset_to_random_kickoff(None);
            },
            0,
        );

        self.extra_car_info.clear();

        for (i, player) in match_settings.player_configurations.iter().enumerate() {
            let team = match player.team {
                0 => Team::BLUE,
                1 => Team::ORANGE,
                _ => unreachable!(),
            };

            let car_config = CarConfig::octane();
            let car_id = self.arena.pin_mut().add_car(team, car_config);
            self.extra_car_info.insert(i, (player.name.clone(), car_id, player.spawn_id));
        }

        self.arena.pin_mut().reset_to_random_kickoff(None);

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

    fn advance_state(&mut self) -> GameState {
        if NEEDS_RESET.load(Ordering::Relaxed) {
            NEEDS_RESET.store(false, Ordering::Relaxed);
            self.set_state_to_countdown();
        }

        if self.state_type == flat::GameStateType::Countdown {
            let ticks_remaining = self.countdown_end_tick - self.arena.get_tick_count();
            if ticks_remaining % 120 == 0 {
                println!("Starting in {}s", ticks_remaining / 120);
            }

            self.countdown_end_tick -= 1;
            if self.countdown_end_tick <= self.arena.get_tick_count() {
                println!("Kickoff!");
                self.state_type = flat::GameStateType::Kickoff
            }
        } else if self.state_type == flat::GameStateType::Kickoff {
            // check and see if the last touch was after the countdown, then advance to "active"
            for car_id in self.arena.pin_mut().get_cars() {
                let state = self.arena.pin_mut().get_car(car_id);

                if !state.ball_hit_info.is_valid {
                    continue;
                }

                if state.ball_hit_info.tick_count_when_hit > self.countdown_end_tick {
                    println!("Ball touched, game is now active");
                    self.state_type = flat::GameStateType::Active;
                    break;
                }
            }

            self.arena.pin_mut().step(1);
        } else if self.state_type == flat::GameStateType::Active {
            self.arena.pin_mut().step(1);
        }

        let game_state = self.arena.pin_mut().get_game_state();

        // construct and send out game tick packet
        let packet = self.make_game_tick_packet(&game_state);

        self.flat_builder.reset();
        let offset = packet.pack(&mut self.flat_builder);
        self.flat_builder.finish(offset, None);
        let bytes = self.flat_builder.finished_data();

        let _ = self.tx.send(messages::FromGame::GameTickPacket(bytes.into()));

        game_state
    }

    fn make_game_tick_packet(&self, game_state: &GameState) -> flat::GameTickPacketT {
        let mut packet = flat::GameTickPacketT::default();

        // Misc
        packet.game_info.seconds_elapsed = game_state.tick_count as f32 * GAME_DT;
        packet.game_info.frame_num = game_state.tick_count as u32;
        packet.game_info.game_state_type = self.state_type;

        // Ball
        packet.ball.physics.location = flat::Vector3T {
            x: game_state.ball.pos.x,
            y: game_state.ball.pos.y,
            z: game_state.ball.pos.z,
        };
        packet.ball.physics.velocity = flat::Vector3T {
            x: game_state.ball.vel.x,
            y: game_state.ball.vel.y,
            z: game_state.ball.vel.z,
        };
        packet.ball.physics.angular_velocity = flat::Vector3T {
            x: game_state.ball.ang_vel.x,
            y: game_state.ball.ang_vel.y,
            z: game_state.ball.ang_vel.z,
        };

        let rot = Angle::from_rotmat(game_state.ball.rot_mat);
        packet.ball.physics.rotation = flat::RotatorT {
            pitch: rot.pitch,
            yaw: rot.yaw,
            roll: rot.roll,
        };

        let mut sphere_shape: Box<flat::SphereShapeT> = Box::default();
        sphere_shape.diameter = self.arena.get_ball_radius() * 2.;
        packet.ball.shape = flat::CollisionShapeT::SphereShape(sphere_shape);

        // Cars
        let mut i = 0;
        packet.players.reserve(self.extra_car_info.len());
        while let Some((name, car_id, spawn_id)) = self.extra_car_info.get(&i).map(Clone::clone) {
            let car = game_state.cars.iter().find(|car| car.id == car_id).unwrap();

            let mut player = flat::PlayerInfoT::default();

            player.physics.location = flat::Vector3T {
                x: car.state.pos.x,
                y: car.state.pos.y,
                z: car.state.pos.z,
            };
            player.physics.velocity = flat::Vector3T {
                x: car.state.vel.x,
                y: car.state.vel.y,
                z: car.state.vel.z,
            };
            player.physics.angular_velocity = flat::Vector3T {
                x: car.state.ang_vel.x,
                y: car.state.ang_vel.y,
                z: car.state.ang_vel.z,
            };

            let rot = Angle::from_rotmat(car.state.rot_mat);
            player.physics.rotation = flat::RotatorT {
                pitch: rot.pitch,
                yaw: rot.yaw,
                roll: rot.roll,
            };

            player.team = car.team as u32;
            player.spawn_id = spawn_id;
            player.is_bot = true;
            player.name = name;
            player.boost = car.state.boost as u32;

            packet.players.push(player);
            i += 1;
        }

        packet
    }
}

#[tokio::main]
pub async fn run_rl(
    tx: broadcast::Sender<messages::FromGame>,
    rx: mpsc::Receiver<messages::ToGame>,
    shutdown_sender: mpsc::Sender<()>,
    headless: bool,
) {
    init(None);

    let interval = interval(Duration::from_secs_f32(GAME_DT));
    let game = Game::new(tx);

    if headless {
        run_headless(interval, game, rx).await;
    } else {
        run_with_rlviser(interval, game, rx).await;
    }

    println!("Shutting down RocketSim");
    shutdown_sender.send(()).await.unwrap();
}

async fn run_with_rlviser(mut interval: Interval, mut game: Game<'_>, mut rx: mpsc::Receiver<messages::ToGame>) {
    let mut rlviser = viser::ExternalManager::new().await.unwrap();

    loop {
        tokio::select! {
            Ok(game_state) = rlviser.check_for_messages() => {
                if let Some(game_state) = game_state {
                    game.set_state(&game_state);
                }
            }
            // modifications below should also be made to the `run_headless` function
            Some(msg) = rx.recv() => {
                if !game.handle_message_from_client(msg).unwrap() {
                    break;
                }
            }
            // make tokio timer that goes off 120 times per second
            // every time it goes off, send a game tick packet to the client
            _ = interval.tick() => {
                let game_state = game.advance_state();
                rlviser.send_game_state(&game_state).await.unwrap();
            },
            else => break,
        }
    }

    rlviser.close().await.unwrap();
}

async fn run_headless(mut interval: Interval, mut game: Game<'_>, mut rx: mpsc::Receiver<messages::ToGame>) {
    loop {
        tokio::select! {
            Some(msg) = rx.recv() => {
                if !game.handle_message_from_client(msg).unwrap() {
                    break;
                }
            }
            _ = interval.tick() => {
                game.advance_state();
            }
            else => break,
        }
    }
}
