use std::{ops::ControlFlow, time::Duration};

use async_timer::Interval;
use rlbot_flat::flat;
use rocketsim::{Arena, ArenaConfig, CarBodyConfig, CarControls, GameMode, Team, Vec3A};
use tokio::select;

use crate::{
    CliArgs,
    connection::{RLBotConnection, connect_rlbot},
    conversion::{GamePacketExt, IntoThat},
};

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
    /// the number of inputs received since the last time step
    num_inputs: u64,
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
            countdown_start: None,
            num_inputs: 0,
            match_length: 0,
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

        if let Some(countdown_start) = self.countdown_start {
            let countdown_elapsed = self.state.match_info.frame_num - countdown_start;
            if countdown_elapsed >= Self::NUM_COUNTDOWN_TICKS {
                println!("Go!");
                self.countdown_start = None;
                self.state.match_info.match_phase = flat::MatchPhase::Kickoff;
            } else if countdown_elapsed.is_multiple_of(u32::from(GameState::TPS)) {
                let time_remaining =
                    (Self::NUM_COUNTDOWN_TICKS - countdown_elapsed) / u32::from(GameState::TPS);
                println!("{time_remaining}...");
            }
        }

        if self.state.match_info.match_phase == flat::MatchPhase::Countdown {
            if self.countdown_start.is_none() {
                println!("Kickoff in 3...");
                self.countdown_start = Some(self.state.match_info.frame_num);
            }

            for idx in 0..arena.num_cars() {
                arena.set_car_controls(idx, CarControls::DEFAULT);
            }
        }

        self.num_inputs = 0;
        self.state.step(arena, match_config, self.match_length);
        self.connection
            .send_packet(self.state.clone())
            .await
            .expect("Failed to send game state");
    }

    pub async fn handle_interface_msg(&mut self, msg: flat::InterfaceMessage) -> ControlFlow<()> {
        match msg {
            flat::InterfaceMessage::MatchConfiguration(match_config) => {
                use rocketsim::consts::{GRAVITY_Z, TICK_TIME};

                assert_eq!(self.state.teams.len(), 2);
                for team in &mut self.state.teams {
                    team.score = 0;
                }

                let match_length = match_config
                    .mutators
                    .as_ref()
                    .map(|m| m.match_length)
                    .unwrap_or(flat::MatchLengthMutator::FiveMinutes);

                self.num_inputs = 0;
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
                self.state.match_info.is_unlimited_time =
                    match_length == flat::MatchLengthMutator::Unlimited;
                self.state.match_info.game_time_remaining =
                    if self.state.match_info.is_unlimited_time {
                        0.0
                    } else {
                        self.match_length as f32 * TICK_TIME
                    };
                self.state.match_info.is_overtime = false;
                self.state.match_info.last_spectated = u32::MAX;
                self.state.match_info.match_phase = flat::MatchPhase::Paused;
                self.state.match_info.world_gravity_z = GRAVITY_Z;

                let game_mode = match_config.game_mode.into_that();

                let mut arena = Arena::new_with_config(game_mode, self.default_config.clone());
                arena.set_vis_enabled(!self.headless);

                assert!(
                    match_config.player_configurations.len() <= 64,
                    "RLBotSim does not support more than 64 players!"
                );
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
                    use rocketsim::consts::goal;

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
                        goals: match game_mode {
                            GameMode::Soccar => vec![
                                flat::GoalInfo {
                                    team_num: 0,
                                    location: goal::get_goal_face_center(Team::Blue).into(),
                                    direction: Vec3A::Y.into(),
                                    width: goal::SOCCAR_GOAL_HALF_WIDTH,
                                    height: goal::SOCCAR_GOAL_HEIGHT / 2.0,
                                },
                                flat::GoalInfo {
                                    team_num: 1,
                                    location: goal::get_goal_face_center(Team::Orange).into(),
                                    direction: Vec3A::NEG_Y.into(),
                                    width: goal::SOCCAR_GOAL_HALF_WIDTH,
                                    height: goal::SOCCAR_GOAL_HEIGHT / 2.0,
                                },
                            ],
                            mode => todo!("Goal info for game mode {mode:?} is not implemented"),
                        },
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

                {
                    use rocketsim::consts::ball;

                    let ball = arena.get_ball_state();
                    let mut ball_shape = Box::<flat::SphereShape>::default();
                    ball_shape.diameter = ball::get_radius(game_mode);

                    self.state.balls.push(flat::BallInfo {
                        physics: flat::Physics {
                            location: ball.pos.into(),
                            velocity: ball.vel.into(),
                            rotation: ball.rot_mat.into_that(),
                            angular_velocity: ball.ang_vel.into(),
                        },
                        shape: flat::CollisionShape::SphereShape(ball_shape),
                    });
                }

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

                if self.lockstep {
                    self.num_inputs |= 1 << input.player_index;

                    if self.num_inputs.count_ones() == arena.num_cars() as u32 {
                        self.step().await;
                    }
                }
            }
            flat::InterfaceMessage::DisconnectSignal(_) => return ControlFlow::Break(()),
            flat::InterfaceMessage::StartCommand(_) => {
                // improvised ready message telling us all the bots have connected
                let Some(match_config) = self.match_config.as_deref() else {
                    return ControlFlow::Continue(());
                };

                // Start the game
                self.state.match_info.match_phase = if match_config.instant_start {
                    flat::MatchPhase::Kickoff
                } else {
                    flat::MatchPhase::Countdown
                };

                if self.lockstep {
                    // Send out the initial game state
                    self.step().await;
                }
            }
            flat::InterfaceMessage::StopCommand(cmd) => {
                if cmd.shutdown_server {
                    return ControlFlow::Break(());
                } else {
                    self.arena = None;
                    self.state.match_info.match_phase = flat::MatchPhase::Ended;
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
