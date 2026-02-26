use std::{ops::ControlFlow, time::Duration};

use async_timer::Interval;
use rlbot_flat::flat::{self, GamePacket, TeamInfo};
use rocketsim::{Arena, ArenaConfig, CarBodyConfig, CarControls, GameMode, Team};
use tokio::select;

use crate::{
    CliArgs,
    connection::{RLBotConnection, connect_rlbot},
};

pub struct GameState {
    connection: RLBotConnection,
    default_config: ArenaConfig,
    arena: Option<Arena>,
    match_config: Option<Box<flat::MatchConfiguration>>,
    headless: bool,
    lockstep: bool,
    /// length of the match in physics ticks
    match_length: u32,
    state: GamePacket,
}

impl GameState {
    const TPS: u8 = 120;
    const DT: f64 = 1.0 / Self::TPS as f64;

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
            match_length: 5 * 60 * u32::from(Self::TPS),
            state: GamePacket {
                balls: Vec::with_capacity(1),
                players: Vec::with_capacity(6),
                match_info: Box::default(),
                boost_pads: Vec::new(),
                teams: vec![
                    TeamInfo {
                        team_index: 0,
                        score: 0,
                    },
                    TeamInfo {
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

        // let Some(match_config) = self.match_config.as_deref_mut() else {
        //     return;
        // };

        let _events = arena.step_tick();

        self.state.match_info.frame_num += 1;
        self.state.match_info.seconds_elapsed =
            (self.state.match_info.frame_num as f64 * Self::DT) as f32;

        if self.match_length >= self.state.match_info.frame_num {
            // count down to the end
            self.state.match_info.game_time_remaining =
                ((self.match_length - self.state.match_info.frame_num) as f64 * Self::DT) as f32;
        } else {
            // we're in overtime, count up
            self.state.match_info.is_overtime = true;
            self.state.match_info.game_time_remaining =
                ((self.state.match_info.frame_num - self.match_length) as f64 * Self::DT) as f32;
        };

        if arena.is_ball_scored() {
            let team_scored = usize::from(arena.get_ball_state().pos.y.is_sign_positive());
            self.state.teams[team_scored].score += 1;

            arena.reset_to_random_kickoff();
        }
    }

    pub async fn handle_interface_msg(&mut self, msg: flat::InterfaceMessage) -> ControlFlow<()> {
        match msg {
            flat::InterfaceMessage::MatchConfiguration(match_config) => {
                assert_eq!(self.state.teams.len(), 2);
                for team in &mut self.state.teams {
                    team.score = 0;
                }

                self.state.players.clear();
                self.state.boost_pads.clear();
                self.state.match_info.seconds_elapsed = 0.0;
                self.state.match_info.frame_num = 0;
                self.state.match_info.game_speed = 1.0;
                self.state.match_info.game_time_remaining =
                    (self.match_length as f64 * Self::DT) as f32;
                self.state.match_info.is_overtime = false;
                self.state.match_info.is_unlimited_time = false;
                self.state.match_info.last_spectated = u32::MAX;
                self.state.match_info.match_phase = flat::MatchPhase::Paused;
                self.state.match_info.world_gravity_z = -650.0;

                let game_mode = match match_config.game_mode {
                    flat::GameMode::Soccar => GameMode::Soccar,
                    flat::GameMode::Hoops => GameMode::Hoops,
                    flat::GameMode::Heatseeker => GameMode::Heatseeker,
                    flat::GameMode::Snowday => GameMode::Snowday,
                    flat::GameMode::Dropshot => GameMode::Dropshot,
                    game_mode => unimplemented!("Unsupported game mode: {:?}", game_mode),
                };

                let mut arena = Arena::new_with_config(game_mode, self.default_config.clone());
                arena.set_vis_enabled(!self.headless);

                for player in &match_config.player_configurations {
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

                self.arena = Some(arena);
                self.match_config = Some(match_config);
            }
            flat::InterfaceMessage::PlayerInput(input) => {
                let Some(arena) = self.arena.as_mut() else {
                    return ControlFlow::Continue(());
                };

                let controls = CarControls {
                    throttle: input.controller_state.throttle,
                    steer: input.controller_state.steer,
                    pitch: input.controller_state.pitch,
                    yaw: input.controller_state.yaw,
                    roll: input.controller_state.roll,
                    jump: input.controller_state.jump,
                    boost: input.controller_state.boost,
                    handbrake: input.controller_state.handbrake,
                };

                arena.set_car_controls(input.player_index as usize, controls);
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
