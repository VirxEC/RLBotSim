use std::{ops::ControlFlow, time::Duration};

use async_timer::Interval;
use rlbot_flat::flat;
use rocketsim::{
    Arena, ArenaConfig, ArenaEvent, CarBodyConfig, CarControls, CarState, GameMode, Mat3A, Team,
    Vec3A,
};
use tokio::select;

use crate::{
    CliArgs,
    connection::{RLBotConnection, connect_rlbot},
};

// custom from/into traits to convert between flat and rocketsim types
trait FromThis<T> {
    fn from_this(value: T) -> Self;
}

trait IntoThat<T> {
    fn into_that(self) -> T;
}

// impl intothat for all types that implement fromthis
impl<T, U> IntoThat<U> for T
where
    U: FromThis<T>,
{
    fn into_that(self) -> U {
        U::from_this(self)
    }
}

impl FromThis<flat::GameMode> for GameMode {
    fn from_this(value: flat::GameMode) -> Self {
        match value {
            flat::GameMode::Soccar => GameMode::Soccar,
            flat::GameMode::Hoops => GameMode::Hoops,
            flat::GameMode::Heatseeker => GameMode::Heatseeker,
            flat::GameMode::Snowday => GameMode::Snowday,
            flat::GameMode::Dropshot => GameMode::Dropshot,
            game_mode => unimplemented!("Unsupported game mode: {:?}", game_mode),
        }
    }
}

impl FromThis<Vec3A> for Box<flat::BoxShape> {
    fn from_this(value: Vec3A) -> Self {
        let mut out = Box::<flat::BoxShape>::default();
        out.length = value.x;
        out.width = value.y;
        out.height = value.z;

        out
    }
}

impl FromThis<&CarState> for flat::AirState {
    fn from_this(value: &CarState) -> Self {
        // todo: figure out how to determine flat::AirState::DoubleJumping
        if value.is_jumping {
            flat::AirState::Jumping
        } else if value.is_auto_flipping || value.is_flipping {
            flat::AirState::Dodging
        } else if value.is_on_ground {
            flat::AirState::OnGround
        } else {
            flat::AirState::InAir
        }
    }
}

impl FromThis<flat::Rotator> for Mat3A {
    fn from_this(rotator: flat::Rotator) -> Self {
        let (sp, cp) = rotator.pitch.sin_cos();
        let (sy, cy) = rotator.yaw.sin_cos();
        let (sr, cr) = rotator.roll.sin_cos();

        Self::from_cols(
            Vec3A::new(cp * cy, cp * sy, sp),
            Vec3A::new(cy * sp * sr - cr * sy, sy * sp * sr + cr * cy, -cp * sr),
            Vec3A::new(-cr * cy * sp - sr * sy, -cr * sy * sp + sr * cy, cp * cr),
        )
    }
}

impl FromThis<Mat3A> for flat::Rotator {
    fn from_this(value: Mat3A) -> Self {
        flat::Rotator {
            pitch: value.x_axis.z.atan2(value.x_axis.x.hypot(value.x_axis.y)),
            yaw: value.x_axis.y.atan2(value.x_axis.x),
            roll: (-value.y_axis.z).atan2(value.z_axis.z),
        }
    }
}

impl FromThis<flat::ControllerState> for CarControls {
    fn from_this(value: flat::ControllerState) -> Self {
        Self {
            throttle: value.throttle,
            steer: value.steer,
            pitch: value.pitch,
            yaw: value.yaw,
            roll: value.roll,
            jump: value.jump,
            boost: value.boost,
            handbrake: value.handbrake,
        }
    }
}

impl FromThis<CarControls> for flat::ControllerState {
    fn from_this(value: CarControls) -> Self {
        Self {
            throttle: value.throttle,
            steer: value.steer,
            pitch: value.pitch,
            yaw: value.yaw,
            roll: value.roll,
            jump: value.jump,
            boost: value.boost,
            handbrake: value.handbrake,
            use_item: false,
        }
    }
}

impl FromThis<Vec3A> for flat::Vector2 {
    fn from_this(value: Vec3A) -> Self {
        Self {
            x: value.x,
            y: value.y,
        }
    }
}

trait GamePacketExt {
    fn step(
        &mut self,
        arena: &mut Arena,
        match_config: &flat::MatchConfiguration,
        match_length: u32,
    );
}

impl GamePacketExt for flat::GamePacket {
    fn step(
        &mut self,
        arena: &mut Arena,
        match_config: &flat::MatchConfiguration,
        match_length: u32,
    ) {
        let events = arena.step_tick();

        for event in events {
            match event {
                ArenaEvent::CarHitBall(info) => {
                    let player = &mut self.players[info.car_idx];
                    let mut touch = Box::<flat::Touch>::default();
                    touch.ball_index = 0;
                    touch.location = info.contact_point.into();
                    touch.normal = info.extra_hit_vel.into();
                    touch.game_seconds = self.match_info.seconds_elapsed;

                    player.latest_touch = Some(touch);
                }
                ArenaEvent::CarHitCar(info) => {
                    if info.is_demo {
                        self.players[info.bumper_car_idx].score_info.demolitions += 1;
                    }
                }
                _ => {}
            }
        }

        self.match_info.frame_num += 1;
        self.match_info.seconds_elapsed = (self.match_info.frame_num as f64 * GameState::DT) as f32;

        if arena.is_ball_scored() {
            let team_scored = usize::from(arena.get_ball_state().pos.y.is_sign_positive());
            self.teams[team_scored].score += 1;
            if self.match_info.is_overtime {
                // end the match immediately if we're in overtime
                self.match_info.match_phase = flat::MatchPhase::Ended;
            }

            arena.reset_to_random_kickoff();
            self.match_info.match_phase = if match_config.instant_start {
                flat::MatchPhase::Kickoff
            } else {
                flat::MatchPhase::Countdown
            };
        }

        if match_length == 0 {
            // unlimited length match, count up
            self.match_info.game_time_remaining =
                (self.match_info.frame_num as f64 * GameState::DT) as f32;
        } else if match_length > self.match_info.frame_num {
            // count down to the end
            self.match_info.game_time_remaining =
                ((match_length - self.match_info.frame_num) as f64 * GameState::DT) as f32;
        } else if match_length == self.match_info.frame_num {
            self.match_info.game_time_remaining = 0.0;

            if self.teams[0].score == self.teams[1].score {
                // if the score is tied, go into overtime instead of ending the match
                self.match_info.is_overtime = true;
                arena.reset_to_random_kickoff();
                self.match_info.match_phase = if match_config.instant_start {
                    flat::MatchPhase::Kickoff
                } else {
                    flat::MatchPhase::Countdown
                };
            } else {
                // otherwise, end the match
                self.match_info.match_phase = flat::MatchPhase::Ended;
            }
        } else {
            // we're in overtime, count up
            self.match_info.is_overtime = true;
            self.match_info.game_time_remaining =
                ((self.match_info.frame_num - match_length) as f64 * GameState::DT) as f32;
        };

        {
            let ball = arena.get_ball_state();
            let phys = &mut self.balls[0].physics;
            phys.location = ball.pos.into();
            phys.velocity = ball.vel.into();
            phys.rotation = ball.rot_mat.into_that();
            phys.angular_velocity = ball.ang_vel.into();
        }

        for pad_idx in 0..arena.num_boost_pads() {
            let state = arena.get_boost_pad_state(pad_idx);
            let pad = &mut self.boost_pads[pad_idx];
            pad.is_active = state.is_active();
            pad.timer = state.cooldown;
        }

        for car_idx in 0..arena.num_cars() {
            let state = arena.get_car_state(car_idx);
            let player = &mut self.players[car_idx];

            let phys = &mut player.physics;
            phys.location = state.pos.into();
            phys.velocity = state.vel.into();
            phys.rotation = state.rot_mat.into_that();
            phys.angular_velocity = state.ang_vel.into();

            player.air_state = state.into_that();
            let dodge_time =
                rocketsim::consts::car::jump::DOUBLEJUMP_MAX_DELAY - state.air_time_since_jump;
            player.dodge_timeout = if state.is_on_ground || dodge_time <= 0.0 {
                -1.0
            } else {
                dodge_time
            };
            player.demolished_timeout = if state.is_demoed {
                state.demo_respawn_timer
            } else {
                -1.0
            };
            player.is_supersonic = state.is_supersonic;
            player.boost = state.boost;
            player.last_input = state.controls.into_that();
            player.has_jumped = state.has_jumped;
            player.has_double_jumped = state.has_double_jumped;
            player.has_dodged = state.has_flipped;
            player.dodge_elapsed = state.flip_time;
            player.dodge_dir = state.flip_rel_torque.normalize_or_zero().into_that();
        }
    }
}

pub struct GameState {
    connection: RLBotConnection,
    default_config: ArenaConfig,
    arena: Option<Arena>,
    match_config: Option<Box<flat::MatchConfiguration>>,
    headless: bool,
    lockstep: bool,
    countdown_start: Option<u32>,
    /// length of the match in physics ticks
    match_length: u32,
    state: flat::GamePacket,
}

impl GameState {
    const TPS: u8 = 120;
    const DT: f64 = 1.0 / Self::TPS as f64;
    const NUM_COUNTDOWN_TICKS: u32 = 3 * Self::TPS as u32;

    pub async fn new(args: CliArgs) -> Self {
        let connection = connect_rlbot(args.rlbot_port).await;

        let default_config = ArenaConfig {
            no_ball_rot: args.headless,
            ..Default::default()
        };

        Self {
            connection,
            default_config,
            arena: None,
            match_config: None,
            headless: args.headless,
            lockstep: args.lockstep,
            countdown_start: Some(0),
            match_length: 5 * 60 * u32::from(Self::TPS),
            state: flat::GamePacket {
                balls: Vec::with_capacity(1),
                players: Vec::with_capacity(6),
                match_info: Box::default(),
                boost_pads: Vec::new(),
                teams: vec![
                    flat::TeamInfo {
                        team_index: 0,
                        score: 0,
                    },
                    flat::TeamInfo {
                        team_index: 1,
                        score: 0,
                    },
                ],
            },
        }
    }

    pub async fn step(&mut self) {
        let Some(arena) = self.arena.as_mut() else {
            return;
        };

        let Some(match_config) = self.match_config.as_deref_mut() else {
            return;
        };

        if matches!(
            self.state.match_info.match_phase,
            flat::MatchPhase::Paused | flat::MatchPhase::Ended | flat::MatchPhase::Inactive
        ) {
            self.connection
                .send_packet(self.state.clone())
                .await
                .expect("Failed to send game state");

            return;
        }

        if let Some(countdown_start) = self.countdown_start
            && self.state.match_info.frame_num - countdown_start >= Self::NUM_COUNTDOWN_TICKS
        {
            self.state.match_info.match_phase = flat::MatchPhase::Kickoff;
        }

        if self.state.match_info.match_phase == flat::MatchPhase::Countdown {
            if self.countdown_start.is_none() {
                self.countdown_start = Some(self.state.match_info.frame_num);
            }

            for idx in 0..arena.num_cars() {
                arena.set_car_controls(idx, CarControls::DEFAULT);
            }
        } else {
            self.countdown_start = None;
        }

        self.state.step(arena, match_config, self.match_length);
        self.connection
            .send_packet(self.state.clone())
            .await
            .expect("Failed to send game state");
    }

    pub async fn handle_interface_msg(&mut self, msg: flat::InterfaceMessage) -> ControlFlow<()> {
        match msg {
            flat::InterfaceMessage::MatchConfiguration(match_config) => {
                assert_eq!(self.state.teams.len(), 2);
                for team in &mut self.state.teams {
                    team.score = 0;
                }

                let match_length = match_config
                    .mutators
                    .as_ref()
                    .map(|m| m.match_length)
                    .unwrap_or(flat::MatchLengthMutator::FiveMinutes);

                self.match_length = match match_length {
                    flat::MatchLengthMutator::FiveMinutes => 5 * 60 * u32::from(Self::TPS),
                    flat::MatchLengthMutator::TenMinutes => 10 * 60 * u32::from(Self::TPS),
                    flat::MatchLengthMutator::TwentyMinutes => 20 * 60 * u32::from(Self::TPS),
                    flat::MatchLengthMutator::Unlimited => 0,
                };

                self.state.balls.clear();
                self.state.players.clear();
                self.state.boost_pads.clear();
                self.state.match_info.seconds_elapsed = 0.0;
                self.state.match_info.frame_num = 0;
                self.state.match_info.game_speed = 1.0;
                self.state.match_info.game_time_remaining =
                    (self.match_length as f64 * Self::DT) as f32;
                self.state.match_info.is_overtime = false;
                self.state.match_info.is_unlimited_time =
                    match_length == flat::MatchLengthMutator::Unlimited;
                self.state.match_info.last_spectated = u32::MAX;
                self.state.match_info.match_phase = flat::MatchPhase::Paused;
                self.state.match_info.world_gravity_z = -650.0;

                let game_mode = match_config.game_mode.into_that();
                let mut arena = Arena::new_with_config(game_mode, self.default_config.clone());
                arena.set_vis_enabled(!self.headless);

                for player in &match_config.player_configurations {
                    match player.variety {
                        flat::PlayerClass::PsyonixBot(_) => {
                            unimplemented!("Psyonix bots will not be implemented")
                        }
                        flat::PlayerClass::Human(_) => todo!("Human players are not supported yet"),
                        flat::PlayerClass::CustomBot(_) => {}
                    }

                    let team = u8::try_from(player.team)
                        .ok()
                        .and_then(|team| Team::try_from(team).ok())
                        .expect("Invalid player team");

                    arena.add_car(team, CarBodyConfig::OCTANE);
                }

                {
                    let field_info = flat::FieldInfo {
                        boost_pads: (0..arena.num_boost_pads())
                            .map(|i| {
                                let config = arena.get_boost_pad_config(i);

                                flat::BoostPad {
                                    location: config.pos.into(),
                                    is_full_boost: config.is_big,
                                }
                            })
                            .collect(),
                        goals: Vec::new(),
                    };

                    self.connection
                        .send_packet(field_info)
                        .await
                        .expect("Failed to send field info");
                }

                arena.reset_to_random_kickoff();

                self.state.boost_pads.reserve(arena.num_boost_pads());
                for pad_idx in 0..arena.num_boost_pads() {
                    let state = arena.get_boost_pad_state(pad_idx);

                    self.state.boost_pads.push(flat::BoostPadState {
                        is_active: state.is_active(),
                        timer: state.cooldown,
                    });
                }

                let ball = arena.get_ball_state();
                let mut ball_shape = Box::<flat::SphereShape>::default();
                ball_shape.diameter = rocketsim::consts::ball::get_radius(game_mode);

                self.state.balls.push(flat::BallInfo {
                    physics: flat::Physics {
                        location: ball.pos.into(),
                        velocity: ball.vel.into(),
                        rotation: ball.rot_mat.into_that(),
                        angular_velocity: ball.ang_vel.into(),
                    },
                    shape: flat::CollisionShape::SphereShape(ball_shape),
                });

                self.state.players.reserve(arena.num_cars());
                for car_idx in 0..arena.num_cars() {
                    let player_info = &match_config.player_configurations[car_idx];
                    let (is_bot, name) = match &player_info.variety {
                        flat::PlayerClass::CustomBot(info) => (true, info.name.clone()),
                        flat::PlayerClass::Human(_) => (false, String::from("Human")),
                        flat::PlayerClass::PsyonixBot(_) => unreachable!(),
                    };

                    let (info, state) = arena.get_car_info_and_state(car_idx);

                    self.state.players.push(flat::PlayerInfo {
                        physics: flat::Physics {
                            location: state.pos.into(),
                            velocity: state.vel.into(),
                            rotation: state.rot_mat.into_that(),
                            angular_velocity: state.ang_vel.into(),
                        },
                        score_info: flat::ScoreInfo::default(),
                        hitbox: info.config.hitbox_size.into_that(),
                        hitbox_offset: info.config.hitbox_pos_offset.into(),
                        latest_touch: None,
                        air_state: state.into_that(),
                        dodge_timeout: -1.0,
                        demolished_timeout: -1.0,
                        is_supersonic: state.is_supersonic,
                        is_bot,
                        name,
                        team: info.team as u32,
                        boost: state.boost,
                        player_id: car_idx as i32,
                        accolades: Vec::new(),
                        last_input: flat::ControllerState::default(),
                        has_jumped: false,
                        has_double_jumped: false,
                        has_dodged: false,
                        dodge_elapsed: 0.0,
                        dodge_dir: flat::Vector2::default(),
                    });
                }

                self.arena = Some(arena);
                self.match_config = Some(match_config);
            }
            flat::InterfaceMessage::PlayerInput(input) => {
                let Some(arena) = self.arena.as_mut() else {
                    return ControlFlow::Continue(());
                };

                arena.set_car_controls(
                    input.player_index as usize,
                    input.controller_state.into_that(),
                );
            }
            flat::InterfaceMessage::DisconnectSignal(_) => return ControlFlow::Break(()),
            flat::InterfaceMessage::StopCommand(cmd) => {
                if cmd.shutdown_server {
                    return ControlFlow::Break(());
                } else {
                    todo!("ability to pause match")
                }
            }
            _ => {}
        }

        ControlFlow::Continue(())
    }

    async fn run_realtime(&mut self) {
        let mut tick_interval = Interval::platform_new(Duration::from_secs_f64(Self::DT));

        loop {
            select! {
                biased;
                _ = tick_interval.wait() => self.step().await,
                Ok(msg) = self.connection.recv_packet() => {
                    if let ControlFlow::Break(_) = self.handle_interface_msg(msg).await {
                        break;
                    }
                }
                else => break,
            }
        }
    }

    async fn run_lockstep(&mut self) {
        while let Ok(msg) = self.connection.recv_packet().await {
            if let ControlFlow::Break(_) = self.handle_interface_msg(msg).await {
                break;
            }
        }
    }

    pub async fn run(&mut self) {
        if self.lockstep {
            self.run_lockstep().await;
        } else {
            self.run_realtime().await;
        }
    }
}
