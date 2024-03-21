use crate::{
    messages,
    util::{self, RsToFlat},
    viser,
};
use async_timer::{interval, Interval};
use rlbot_sockets::{flat, flatbuffers::FlatBufferBuilder};
use rocketsim_rs::{
    consts::DOUBLEJUMP_MAX_DELAY,
    cxx::UniquePtr,
    init,
    sim::{Arena, CarConfig, CarControls, Team},
    GameState,
};
use std::{
    collections::HashMap,
    io::Result as IoResult,
    sync::atomic::{AtomicBool, AtomicU32, Ordering},
    time::Duration,
};
use tokio::sync::{broadcast, mpsc};

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
    #[inline]
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
        self.countdown_end_tick = self.arena.get_tick_count() + 3 * u64::from(GAME_TPS);
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

                if let Some((_, match_settings)) = &self.match_settings {
                    self.tx
                        .send(messages::FromGame::MatchSettings(match_settings.clone()))
                        .unwrap();
                }

                if let Some(field_info) = &self.field_info {
                    self.tx.send(messages::FromGame::FieldInfo(field_info.clone())).unwrap();
                }
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

                self.tx.send(messages::FromGame::StopCommand(info.shutdown_server)).unwrap();

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
                self.state_type = flat::GameStateType::Kickoff;
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
        packet.game_info.game_speed = 1.;
        packet.game_info.is_unlimited_time = true;
        packet.game_info.is_overtime = false;
        packet.game_info.world_gravity_z = -650.;
        packet.game_info.seconds_elapsed = game_state.tick_count as f32 * GAME_DT;
        packet.game_info.frame_num = game_state.tick_count as u32;
        packet.game_info.game_state_type = self.state_type;

        // teams
        packet.teams.reserve(2);

        let mut blue_team = flat::TeamInfoT::default();
        blue_team.score = BLUE_SCORE.load(Ordering::Relaxed);
        blue_team.team_index = 0;
        packet.teams.push(blue_team);

        let mut orange_team = flat::TeamInfoT::default();
        orange_team.score = ORANGE_SCORE.load(Ordering::Relaxed);
        orange_team.team_index = 1;
        packet.teams.push(orange_team);

        // boost pad states
        packet.boost_pad_states.reserve(game_state.pads.len());

        for pad in &game_state.pads {
            let mut boost_pad_state = flat::BoostPadStateT::default();
            boost_pad_state.is_active = pad.state.is_active;
            boost_pad_state.timer = pad.state.cooldown;
            packet.boost_pad_states.push(boost_pad_state);
        }

        // Ball
        packet.ball.physics.location = game_state.ball.pos.to_flat();
        packet.ball.physics.velocity = game_state.ball.pos.to_flat();
        packet.ball.physics.angular_velocity = game_state.ball.pos.to_flat();
        packet.ball.physics.rotation = game_state.ball.rot_mat.to_flat();

        let mut sphere_shape: Box<flat::SphereShapeT> = Box::default();
        sphere_shape.diameter = self.arena.get_ball_radius() * 2.;
        packet.ball.shape = flat::CollisionShapeT::SphereShape(sphere_shape);

        let mut last_ball_touch_time = 0;
        let mut last_car = None;

        for car in &game_state.cars {
            if !car.state.ball_hit_info.is_valid {
                continue;
            }

            if car.state.ball_hit_info.tick_count_when_hit > last_ball_touch_time {
                last_car = Some(car);
                last_ball_touch_time = car.state.ball_hit_info.tick_count_when_hit;
            }
        }

        if let Some(car) = last_car {
            packet.ball.latest_touch.location = flat::Vector3T {
                x: car.state.ball_hit_info.relative_pos_on_ball.x + car.state.pos.x,
                y: car.state.ball_hit_info.relative_pos_on_ball.y + car.state.pos.y,
                z: car.state.ball_hit_info.relative_pos_on_ball.z + car.state.pos.z,
            };
            packet.ball.latest_touch.normal = car.state.ball_hit_info.extra_hit_vel.to_flat();
            packet.ball.latest_touch.game_seconds = last_ball_touch_time as f32 * GAME_DT;
            packet.ball.latest_touch.team = car.team as u32;

            let (&index, name, _) = self
                .extra_car_info
                .iter()
                .map(|(index, (name, car_id, _))| (index, name, *car_id))
                .find(|(_, _, car_id)| *car_id == car.id)
                .unwrap();

            packet.ball.latest_touch.player_index = index as u32;
            packet.ball.latest_touch.player_name.clone_from(name);
        }

        // Cars
        let mut i = 0;
        packet.players.reserve(self.extra_car_info.len());
        while let Some((name, car_id, spawn_id)) = self.extra_car_info.get(&i).cloned() {
            let car = game_state.cars.iter().find(|car| car.id == car_id).unwrap();

            let mut player = flat::PlayerInfoT::default();

            player.physics.location = car.state.pos.to_flat();
            player.physics.velocity = car.state.vel.to_flat();
            player.physics.angular_velocity = car.state.ang_vel.to_flat();
            player.physics.rotation = car.state.rot_mat.to_flat();

            player.team = car.team as u32;
            player.spawn_id = spawn_id;
            player.is_bot = true;
            player.name = name;
            player.boost = car.state.boost as u32;
            player.is_supersonic =
                (car.state.pos.x.powi(2) + car.state.pos.y.powi(2) + car.state.pos.z.powi(2)) > 2200f32.powi(2);

            player.hitbox = car.config.hitbox_size.to_flat();
            player.hitbox_offset = car.config.hitbox_pos_offset.to_flat();

            player.demolished_timeout = car.state.demo_respawn_timer;
            player.dodge_timeout = DOUBLEJUMP_MAX_DELAY - car.state.air_time_since_jump;

            player.air_state = if car.state.has_contact {
                flat::AirState::OnGround
            } else if car.state.is_jumping {
                if car.state.has_jumped {
                    flat::AirState::DoubleJumping
                } else {
                    flat::AirState::Jumping
                }
            } else if car.state.is_flipping {
                flat::AirState::Dodging
            } else {
                flat::AirState::InAir
            };

            packet.players.push(player);
            i += 1;
        }

        packet
    }

    async fn run_with_rlviser(mut self, mut interval: Interval, mut rx: mpsc::Receiver<messages::ToGame>) {
        let mut rlviser = viser::ExternalManager::new().await.unwrap();

        loop {
            tokio::select! {
                biased;
                // make tokio timer that goes off 120 times per second
                // every time it goes off, send a game tick packet to the client
                () = interval.wait() => {
                    let game_state = self.advance_state();
                    rlviser.send_game_state(&game_state).await.unwrap();
                },
                // modifications below should also be made to the `run_headless` function
                Some(msg) = rx.recv() => {
                    if !self.handle_message_from_client(msg).unwrap() {
                        break;
                    }
                }
                Ok(game_state) = rlviser.check_for_messages() => {
                    if let Some(game_state) = game_state {
                        self.set_state(&game_state);
                    }
                }
                else => break,
            }
        }

        rlviser.close().await.unwrap();
    }

    async fn run_headless(mut self, mut interval: Interval, mut rx: mpsc::Receiver<messages::ToGame>) {
        loop {
            tokio::select! {
                biased;
                () = interval.wait() => {
                    self.advance_state();
                }
                Some(msg) = rx.recv() => {
                    if !self.handle_message_from_client(msg).unwrap() {
                        break;
                    }
                }
                else => break,
            }
        }
    }
}

#[tokio::main(worker_threads = 2)]
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
        game.run_headless(interval, rx).await;
    } else {
        game.run_with_rlviser(interval, rx).await;
    }

    println!("Shutting down RocketSim");
    shutdown_sender.send(()).await.unwrap();
}
