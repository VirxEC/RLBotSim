use crate::{
    agent_res::AgentReservation,
    messages,
    util::{self, FlatToRs, RsToFlat, SetFromPartial},
    viser,
};
use async_timer::{interval, Interval};
use rlbot_sockets::{
    flat::{self, BallInfoT, ControllerStateT},
    flatbuffers::FlatBufferBuilder,
};
use rocketsim_rs::{
    consts::DOUBLEJUMP_MAX_DELAY,
    cxx::UniquePtr,
    init,
    render::RenderMessage,
    sim::{Arena, BallState, CarConfig, CarControls, Team},
    GameState,
};
use std::{
    collections::HashMap,
    io::Result as IoResult,
    sync::atomic::{AtomicBool, AtomicU32, Ordering},
    time::Duration,
};
use tokio::sync::{broadcast, mpsc};

const PREDICTION_SECONDS: usize = 6;
const GAME_TPS: u8 = 120;
const GAME_DT: f32 = 1. / GAME_TPS as f32;

static BLUE_SCORE: AtomicU32 = AtomicU32::new(0);
static ORANGE_SCORE: AtomicU32 = AtomicU32::new(0);
static NEEDS_RESET: AtomicBool = AtomicBool::new(false);

struct PacketData {
    flat: flat::GamePacketT,
    status: flat::GameStatus,
    extra_car_info: HashMap<usize, (String, u32, i32), ahash::RandomState>,
}

impl PacketData {
    fn new() -> Self {
        let mut flat = flat::GamePacketT::default();
        flat.teams.resize(2, flat::TeamInfoT::default());
        flat.teams[1].team_index = 1;

        Self {
            flat,
            status: flat::GameStatus::Inactive,
            extra_car_info: HashMap::default(),
        }
    }

    #[inline]
    fn get_car_id_from_index(&self, player_index: usize) -> u32 {
        self.extra_car_info[&player_index].1
    }

    #[inline]
    fn clear_extra_car_info(&mut self) {
        self.extra_car_info.clear();
    }

    #[inline]
    fn add_extra_car_info(&mut self, index: usize, name: String, car_id: u32, spawn_id: i32) {
        self.extra_car_info.insert(index, (name, car_id, spawn_id));
    }

    #[inline]
    fn set_state_type(&mut self, state_type: flat::GameStatus) {
        self.status = state_type;
    }

    #[inline]
    const fn get_state_type(&self) -> flat::GameStatus {
        self.status
    }

    fn get_game_tick_packet(
        &mut self,
        game_state: &GameState,
        ball_radius: f32,
    ) -> &flat::GamePacketT {
        // Misc
        self.flat.game_info.game_speed = 1.;
        self.flat.game_info.is_unlimited_time = true;
        self.flat.game_info.is_overtime = false;
        self.flat.game_info.world_gravity_z = -650.;
        self.flat.game_info.seconds_elapsed = game_state.tick_count as f32 * GAME_DT;
        self.flat.game_info.frame_num = game_state.tick_count as u32;
        self.flat.game_info.game_status = self.status;

        // teams
        self.flat.teams[0].score = BLUE_SCORE.load(Ordering::Relaxed);
        self.flat.teams[1].score = ORANGE_SCORE.load(Ordering::Relaxed);

        // boost pad states
        self.flat
            .boost_pads
            .resize_with(game_state.pads.len(), Default::default);

        for (flat_pad, rs_pad) in self.flat.boost_pads.iter_mut().zip(&game_state.pads) {
            flat_pad.is_active = rs_pad.state.is_active;
            flat_pad.timer = rs_pad.state.cooldown;
        }

        // Ball
        let mut ball = BallInfoT::default();
        ball.physics.location = game_state.ball.pos.to_flat();
        ball.physics.velocity = game_state.ball.vel.to_flat();
        ball.physics.angular_velocity = game_state.ball.ang_vel.to_flat();
        ball.physics.rotation = game_state.ball.rot_mat.to_flat();

        let mut sphere_shape = Box::<flat::SphereShapeT>::default();
        sphere_shape.diameter = ball_radius * 2.;
        ball.shape = flat::CollisionShapeT::SphereShape(sphere_shape);

        if self.flat.balls.is_empty() {
            self.flat.balls.push(ball);
        } else {
            self.flat.balls[0] = ball;
        }

        // Cars
        let mut i = 0;
        self.flat
            .players
            .resize_with(self.extra_car_info.len(), Default::default);
        while let Some((name, car_id, spawn_id)) = self.extra_car_info.get(&i).cloned() {
            let car = game_state.cars.iter().find(|car| car.id == car_id).unwrap();

            let player = &mut self.flat.players[i];

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
                (car.state.pos.x.powi(2) + car.state.pos.y.powi(2) + car.state.pos.z.powi(2))
                    > 2200f32.powi(2);

            player.hitbox = car.config.hitbox_size.to_flat();
            player.hitbox_offset = car.config.hitbox_pos_offset.to_flat();

            player.demolished_timeout = car.state.demo_respawn_timer;
            player.dodge_timeout = DOUBLEJUMP_MAX_DELAY - car.state.air_time_since_jump;

            player.air_state = if car.state.is_on_ground {
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

            player.latest_touch = if car.state.ball_hit_info.is_valid {
                let mut hit_info = Box::<flat::TouchT>::default();
                hit_info.location = flat::Vector3T {
                    x: car.state.ball_hit_info.relative_pos_on_ball.x + car.state.pos.x,
                    y: car.state.ball_hit_info.relative_pos_on_ball.y + car.state.pos.y,
                    z: car.state.ball_hit_info.relative_pos_on_ball.z + car.state.pos.z,
                };
                hit_info.normal = car.state.ball_hit_info.extra_hit_vel.to_flat();
                hit_info.game_seconds =
                    car.state.ball_hit_info.tick_count_when_hit as f32 * GAME_DT;
                hit_info.ball_index = 0;

                Some(hit_info)
            } else {
                None
            };

            player.has_jumped = car.state.has_jumped;
            player.has_double_jumped = car.state.has_double_jumped;
            player.has_dodged = car.state.has_flipped;
            player.dodge_elapsed = car.state.flip_time;

            player.last_input = ControllerStateT::default();
            player.last_input.throttle = car.state.last_controls.throttle;
            player.last_input.steer = car.state.last_controls.steer;
            player.last_input.roll = car.state.last_controls.roll;
            player.last_input.pitch = car.state.last_controls.pitch;
            player.last_input.yaw = car.state.last_controls.yaw;
            player.last_input.boost = car.state.last_controls.boost;
            player.last_input.jump = car.state.last_controls.jump;
            player.last_input.handbrake = car.state.last_controls.handbrake;

            i += 1;
        }

        &self.flat
    }
}

struct BallPredData {
    arena: UniquePtr<Arena>,
    flat: flat::BallPredictionT,
}

impl BallPredData {
    fn new() -> Self {
        let mut flat = flat::BallPredictionT::default();
        flat.slices = (0..PREDICTION_SECONDS * GAME_TPS as usize)
            .map(|_| flat::PredictionSliceT::default())
            .collect();

        Self {
            arena: Arena::default_standard(),
            flat,
        }
    }

    fn set_game_mode(&mut self, arena_type: flat::GameMode) {
        self.arena = match arena_type {
            flat::GameMode::Soccer => Arena::default_standard(),
            flat::GameMode::Hoops => Arena::default_hoops(),
            flat::GameMode::Heatseeker => Arena::default_heatseeker(),
            // flat::GameMode::Hockey => Arena::default_snowday(),
            _ => unimplemented!(),
        };
    }

    fn get_ball_prediction(
        &mut self,
        current_ball: BallState,
        mut num_ticks: u64,
    ) -> &flat::BallPredictionT {
        self.arena.pin_mut().set_ball(current_ball);

        for slice in &mut self.flat.slices {
            self.arena.pin_mut().step(1);
            num_ticks += 1;

            slice.game_seconds = num_ticks as f32 * GAME_DT;

            let ball_state = self.arena.pin_mut().get_ball();
            slice.physics.location = ball_state.pos.to_flat();
            slice.physics.velocity = ball_state.vel.to_flat();
            slice.physics.angular_velocity = ball_state.ang_vel.to_flat();
            slice.physics.rotation = ball_state.rot_mat.to_flat();
        }

        &self.flat
    }
}

enum ClientState {
    Connected,
    Disconnected,
    Render(RenderMessage),
}

struct Game<'a> {
    arena: UniquePtr<Arena>,
    tx: broadcast::Sender<messages::FromGame>,
    flat_builder: FlatBufferBuilder<'a>,
    countdown_end_tick: u64,
    match_settings: Option<(flat::MatchSettingsT, Box<[u8]>)>,
    field_info: Option<Box<[u8]>>,
    ball_prediction: BallPredData,
    packet: PacketData,
    agent_reservation: AgentReservation,
}

impl<'a> Game<'a> {
    #[inline]
    fn new(tx: broadcast::Sender<messages::FromGame>) -> Self {
        Self {
            tx,
            arena: Arena::default_standard(),
            flat_builder: FlatBufferBuilder::with_capacity(10240),
            countdown_end_tick: 0,
            match_settings: None,
            field_info: None,
            ball_prediction: BallPredData::new(),
            packet: PacketData::new(),
            agent_reservation: AgentReservation::default(),
        }
    }

    fn set_state_to_countdown(&mut self) {
        self.packet.set_state_type(flat::GameStatus::Countdown);
        self.countdown_end_tick = self.arena.get_tick_count() + 3 * u64::from(GAME_TPS);
    }

    fn handle_message_from_client(&mut self, msg: messages::ToGame) -> IoResult<ClientState> {
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
                util::auto_start_bots(&match_settings)?;
                self.set_match_settings(match_settings);
                self.set_field_info();

                if let Some((_, match_settings)) = &self.match_settings {
                    self.tx
                        .send(messages::FromGame::MatchSettings(match_settings.clone()))
                        .unwrap();
                }

                if let Some(field_info) = &self.field_info {
                    self.tx
                        .send(messages::FromGame::FieldInfo(field_info.clone()))
                        .unwrap();
                }
            }
            messages::ToGame::PlayerInput(input) => {
                let car_id = self
                    .packet
                    .get_car_id_from_index(input.player_index as usize);
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

                self.arena
                    .pin_mut()
                    .set_car_controls(car_id, car_controls)
                    .unwrap();
            }
            messages::ToGame::DesiredGameState(desired_state) => {
                let mut game_state = self.arena.pin_mut().get_game_state();

                if let Some(ball) = desired_state.ball_states.into_iter().next() {
                    let phys = ball.physics;
                    game_state.ball.pos.set_from_partial(phys.location);
                    game_state.ball.vel.set_from_partial(phys.velocity);
                    game_state
                        .ball
                        .ang_vel
                        .set_from_partial(phys.angular_velocity);
                    game_state.ball.rot_mat.set_from_partial(phys.rotation);
                }

                for (i, car) in desired_state.car_states.into_iter().enumerate() {
                    let car_id = self.packet.get_car_id_from_index(i);
                    let car_state = game_state
                        .cars
                        .iter_mut()
                        .find(|car| car.id == car_id)
                        .unwrap();

                    if let Some(phys) = car.physics {
                        car_state.state.pos.set_from_partial(phys.location);
                        car_state.state.vel.set_from_partial(phys.velocity);
                        car_state
                            .state
                            .ang_vel
                            .set_from_partial(phys.angular_velocity);
                        car_state.state.rot_mat.set_from_partial(phys.rotation);
                    }

                    if let Some(boost_amount) = car.boost_amount {
                        car_state.state.boost = boost_amount.val;
                    }
                }

                self.set_state(&game_state);

                if let Some(game_info) = desired_state.game_info_state {
                    if let Some(gravity_z) = game_info.world_gravity_z {
                        let mut mutators = self.arena.get_mutator_config();
                        mutators.gravity.z = gravity_z.val;
                        self.arena.pin_mut().set_mutator_config(mutators);
                    }

                    if let Some(paused) = game_info.paused {
                        self.packet.set_state_type(if paused.val {
                            flat::GameStatus::Paused
                        } else {
                            flat::GameStatus::Active
                        });
                    }
                }
            }
            messages::ToGame::RenderGroup(group) => {
                return Ok(ClientState::Render(group.to_rs()));
            }
            messages::ToGame::RemoveRenderGroup(group) => {
                return Ok(ClientState::Render(group.to_rs()));
            }
            messages::ToGame::MatchComm(message) => {
                self.tx
                    .send(messages::FromGame::MatchComm(message))
                    .unwrap();
            }
            messages::ToGame::StopCommand(info) => {
                self.packet.set_state_type(flat::GameStatus::Ended);

                self.tx
                    .send(messages::FromGame::StopCommand(info.shutdown_server))
                    .unwrap();

                if info.shutdown_server {
                    return Ok(ClientState::Disconnected);
                }
            }
            messages::ToGame::ControllableTeamInfoRequest(agent_id, tx) => {
                let msg = if let Some(team_controllable_info) =
                    self.agent_reservation.reserve_player(&agent_id)
                {
                    self.flat_builder.reset();
                    let offset = team_controllable_info.pack(&mut self.flat_builder);
                    self.flat_builder.finish(offset, None);
                    Some(self.flat_builder.finished_data().into())
                } else {
                    None
                };

                tx.send(msg).unwrap();
            }
        }

        Ok(ClientState::Connected)
    }

    fn set_state(&mut self, game_state: &GameState) {
        self.arena.pin_mut().set_game_state(game_state).unwrap();
    }

    fn set_match_settings(&mut self, match_settings: flat::MatchSettingsT) {
        self.arena = match match_settings.game_mode {
            flat::GameMode::Soccer => Arena::default_standard(),
            flat::GameMode::Hoops => Arena::default_hoops(),
            flat::GameMode::Heatseeker => Arena::default_heatseeker(),
            _ => unimplemented!(),
        };

        self.set_state_to_countdown();

        self.ball_prediction.set_game_mode(match_settings.game_mode);

        self.arena.pin_mut().set_goal_scored_callback(
            |arena, car_team, _| {
                NEEDS_RESET.store(true, Ordering::Relaxed);

                match car_team {
                    Team::Blue => {
                        BLUE_SCORE.fetch_add(1, Ordering::Relaxed);
                    }
                    Team::Orange => {
                        ORANGE_SCORE.fetch_add(1, Ordering::Relaxed);
                    }
                }

                arena.reset_to_random_kickoff(None);
            },
            0,
        );

        self.agent_reservation.set_players(&match_settings);
        self.packet.clear_extra_car_info();

        for (i, player) in match_settings.player_configurations.iter().enumerate() {
            let team = match player.team {
                0 => Team::Blue,
                1 => Team::Orange,
                _ => unreachable!(),
            };

            let car_config = CarConfig::octane();
            let car_id = self.arena.pin_mut().add_car(team, car_config);
            self.packet
                .add_extra_car_info(i, player.name.clone(), car_id, player.spawn_id);
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
        blue_goal.direction = flat::Vector3T {
            x: 0.,
            y: 1.,
            z: 0.,
        };
        blue_goal.width = 892.755;
        blue_goal.height = 642.775;

        let mut orange_goal = flat::GoalInfoT::default();
        orange_goal.team_num = 1;
        orange_goal.location = flat::Vector3T {
            x: 0.,
            y: 5120.,
            z: 642.775,
        };
        orange_goal.direction = flat::Vector3T {
            x: 0.,
            y: -1.,
            z: 0.,
        };
        orange_goal.width = 892.755;
        orange_goal.height = 642.775;

        field_info.goals.reserve(2);
        field_info.goals.push(blue_goal);
        field_info.goals.push(orange_goal);

        field_info
            .boost_pads
            .extend(self.arena.iter_pad_config().map(|conf| {
                let mut boost_pad = flat::BoostPadT::default();

                boost_pad.is_full_boost = conf.is_big;
                boost_pad.location = flat::Vector3T {
                    x: conf.position.x,
                    y: conf.position.y,
                    z: conf.position.z,
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

        if self.packet.get_state_type() == flat::GameStatus::Countdown {
            let ticks_remaining = self.countdown_end_tick - self.arena.get_tick_count();
            if ticks_remaining % 120 == 0 {
                println!("Starting in {}s", ticks_remaining / 120);
            }

            self.countdown_end_tick -= 1;
            if self.countdown_end_tick <= self.arena.get_tick_count() {
                println!("Kickoff!");
                self.packet.set_state_type(flat::GameStatus::Kickoff);
            }
        } else if self.packet.get_state_type() == flat::GameStatus::Kickoff {
            // check and see if the last touch was after the countdown, then advance to "active"
            for car_id in self.arena.pin_mut().get_cars() {
                let state = self.arena.pin_mut().get_car(car_id);

                if !state.ball_hit_info.is_valid {
                    continue;
                }

                if state.ball_hit_info.tick_count_when_hit > self.countdown_end_tick {
                    println!("Ball touched, game is now active");
                    self.packet.set_state_type(flat::GameStatus::Active);
                    break;
                }
            }

            self.arena.pin_mut().step(1);
        } else if self.packet.get_state_type() == flat::GameStatus::Active {
            self.arena.pin_mut().step(1);
        }

        let game_state = self.arena.pin_mut().get_game_state();

        {
            // construct and send out game tick packet
            let packet = self
                .packet
                .get_game_tick_packet(&game_state, self.arena.get_ball_radius());

            self.flat_builder.reset();
            let offset = packet.pack(&mut self.flat_builder);
            self.flat_builder.finish(offset, None);
            let bytes = self.flat_builder.finished_data();

            let _ = self
                .tx
                .send(messages::FromGame::GameTickPacket(bytes.into()));
        }

        {
            let ball_prediction = self
                .ball_prediction
                .get_ball_prediction(game_state.ball, game_state.tick_count);

            self.flat_builder.reset();
            let offset = ball_prediction.pack(&mut self.flat_builder);
            self.flat_builder.finish(offset, None);
            let bytes = self.flat_builder.finished_data();

            let _ = self
                .tx
                .send(messages::FromGame::BallPrediction(bytes.into()));
        }

        game_state
    }

    #[tokio::main(worker_threads = 2)]
    async fn run_with_rlviser(
        mut self,
        mut timer: Interval,
        mut rx: mpsc::Receiver<messages::ToGame>,
    ) {
        let mut rlviser = viser::ExternalManager::new().await.unwrap();

        loop {
            tokio::select! {
                biased;
                // make tokio timer that goes off 120 times per second
                // every time it goes off, send a game tick packet to the client
                () = timer.wait() => {
                    let game_state = self.advance_state();
                    rlviser.send_game_state(&game_state).await.unwrap();
                },
                // modifications below should also be made to the `run_headless` function
                Some(msg) = rx.recv() => {
                    match self.handle_message_from_client(msg).unwrap() {
                        ClientState::Disconnected => break,
                        ClientState::Connected => {}
                        ClientState::Render(render) => {
                            rlviser.send_render_group(render).await.unwrap();
                        }
                    }
                }
                Ok(game_state) = rlviser.check_for_messages() => {
                    match game_state {
                        viser::StateControl::GameState(game_state) => {
                            self.set_state(&game_state);
                        }
                        viser::StateControl::Speed(speed) => {
                            timer = interval(Duration::from_secs_f32(1. / (GAME_TPS as f32 * speed)));
                        }
                        viser::StateControl::Paused(paused) => {
                            self.packet.set_state_type(if paused { flat::GameStatus::Paused } else { flat::GameStatus::Active });
                        }
                        viser::StateControl::None => {}
                    }
                }
                else => break,
            }
        }

        rlviser.close().await.unwrap();
    }

    #[tokio::main(worker_threads = 2)]
    async fn run_headless(
        mut self,
        mut interval: Interval,
        mut rx: mpsc::Receiver<messages::ToGame>,
    ) {
        loop {
            tokio::select! {
                biased;
                () = interval.wait() => {
                    self.advance_state();
                }
                Some(msg) = rx.recv() => {
                    match self.handle_message_from_client(msg).unwrap() {
                        ClientState::Disconnected => break,
                        ClientState::Connected | ClientState::Render(_) => {}
                    }
                }
                else => break,
            }
        }
    }
}

pub fn run_rl(
    tx: broadcast::Sender<messages::FromGame>,
    rx: mpsc::Receiver<messages::ToGame>,
    shutdown_sender: mpsc::Sender<()>,
    headless: bool,
) {
    init(None, cfg!(not(debug_assertions)));

    let interval = interval(Duration::from_secs_f32(GAME_DT));
    let game = Game::new(tx);

    if headless {
        game.run_headless(interval, rx);
    } else {
        game.run_with_rlviser(interval, rx);
    }

    println!("Shutting down RocketSim");
    shutdown_sender.blocking_send(()).unwrap();
}
