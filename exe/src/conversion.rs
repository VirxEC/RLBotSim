use rlbot_flat::flat;
use rocketsim::{Arena, ArenaEvent, CarControls, CarState, GameMode, Mat3A, Vec3A};

// custom from/into traits to convert between flat and rocketsim types
pub trait FromThis<T> {
    fn from_this(value: T) -> Self;
}

pub trait IntoThat<T> {
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

pub trait GamePacketExt {
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
        use rocketsim::consts::TICK_TIME;

        let events = arena.step_tick();

        self.match_info.frame_num += 1;
        self.match_info.seconds_elapsed = self.match_info.frame_num as f32 * TICK_TIME;

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

                    if self.match_info.match_phase == flat::MatchPhase::Kickoff {
                        self.match_info.match_phase = flat::MatchPhase::Active;
                    }
                }
                ArenaEvent::CarHitCar(info) => {
                    if info.is_demo {
                        self.players[info.bumper_car_idx].score_info.demolitions += 1;
                    }
                }
                _ => {}
            }
        }

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
            self.match_info.game_time_remaining = self.match_info.seconds_elapsed;
        } else if match_length > self.match_info.frame_num {
            // count down to the end
            self.match_info.game_time_remaining =
                (match_length - self.match_info.frame_num) as f32 * TICK_TIME;
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
                (self.match_info.frame_num - match_length) as f32 * TICK_TIME;
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
            use rocketsim::consts::car;

            let state = arena.get_car_state(car_idx);
            let player = &mut self.players[car_idx];

            let phys = &mut player.physics;
            phys.location = state.pos.into();
            phys.velocity = state.vel.into();
            phys.rotation = state.rot_mat.into_that();
            phys.angular_velocity = state.ang_vel.into();

            player.air_state = state.into_that();
            let dodge_time = car::jump::DOUBLEJUMP_MAX_DELAY - state.air_time_since_jump;
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
