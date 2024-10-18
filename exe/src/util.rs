use rlbot_sockets::flat;
use rocketsim_rs::{
    math::{Angle, RotMat, Vec3},
    render::{Color, Render, RenderMessage},
};
use std::{io::Result as IoResult, process::Command};

pub fn auto_start_bots(match_settings: &flat::MatchSettingsT) -> IoResult<()> {
    if !match_settings.auto_start_bots {
        return Ok(());
    }

    for player in &match_settings.player_configurations {
        let mut command = Command::new(if cfg!(windows) { "cmd.exe" } else { "/bin/sh" });

        command.env("RLBOT_AGENT_ID", &player.agent_id);
        command.current_dir(&player.root_dir);
        command.args([if cfg!(windows) { "/c" } else { "-c" }, &player.run_command]);

        command.spawn()?;
    }

    Ok(())
}

pub trait RsToFlat<T> {
    fn to_flat(self) -> T;
}

impl RsToFlat<Box<flat::BoxShapeT>> for Vec3 {
    fn to_flat(self) -> Box<flat::BoxShapeT> {
        let mut box_shape = Box::<flat::BoxShapeT>::default();

        box_shape.length = self.x;
        box_shape.width = self.y;
        box_shape.height = self.z;

        box_shape
    }
}

impl RsToFlat<flat::Vector3T> for Vec3 {
    #[inline]
    fn to_flat(self) -> flat::Vector3T {
        flat::Vector3T {
            x: self.x,
            y: self.y,
            z: self.z,
        }
    }
}

impl RsToFlat<flat::RotatorT> for RotMat {
    fn to_flat(self) -> flat::RotatorT {
        let angles = Angle::from_rotmat(self);

        flat::RotatorT {
            pitch: angles.pitch,
            yaw: angles.yaw,
            roll: angles.roll,
        }
    }
}

pub trait SetFromPartial<T> {
    fn set_from_partial(&mut self, partial: Option<T>);
}

impl SetFromPartial<Box<flat::Vector3PartialT>> for Vec3 {
    fn set_from_partial(&mut self, partial: Option<Box<flat::Vector3PartialT>>) {
        if let Some(partial) = partial {
            if let Some(x) = partial.x {
                self.x = x.val;
            }

            if let Some(y) = partial.y {
                self.y = y.val;
            }

            if let Some(z) = partial.z {
                self.z = z.val;
            }
        }
    }
}

impl SetFromPartial<Box<flat::RotatorPartialT>> for RotMat {
    fn set_from_partial(&mut self, partial: Option<Box<flat::RotatorPartialT>>) {
        if let Some(partial) = partial {
            let mut angles = Angle::from_rotmat(*self);

            if let Some(pitch) = partial.pitch {
                angles.pitch = pitch.val;
            }

            if let Some(yaw) = partial.yaw {
                angles.yaw = yaw.val;
            }

            if let Some(roll) = partial.roll {
                angles.roll = roll.val;
            }

            *self = angles.to_rotmat();
        }
    }
}

pub trait FlatToRs<T> {
    fn to_rs(self) -> T;
}

impl FlatToRs<Vec3> for flat::Vector3T {
    fn to_rs(self) -> Vec3 {
        Vec3::new(self.x, self.y, self.z)
    }
}

impl FlatToRs<Color> for flat::ColorT {
    fn to_rs(self) -> Color {
        Color::rgba(
            f32::from(self.r) / 255.,
            f32::from(self.g) / 255.,
            f32::from(self.b) / 255.,
            f32::from(self.a) / 255.,
        )
    }
}

impl FlatToRs<Render> for flat::RenderMessageT {
    fn to_rs(self) -> Render {
        match self.variety {
            flat::RenderTypeT::NONE => panic!("Invalid render type NONE"),
            flat::RenderTypeT::Line3D(line) => Render::Line {
                start: line.start.world.to_rs(),
                end: line.end.world.to_rs(),
                color: line.color.to_rs(),
            },
            flat::RenderTypeT::PolyLine3D(polyline) => {
                let positions = polyline.points.into_iter().map(FlatToRs::to_rs).collect();

                Render::LineStrip {
                    positions,
                    color: polyline.color.to_rs(),
                }
            }
            _ => unimplemented!(),
        }
    }
}

impl FlatToRs<RenderMessage> for flat::RenderGroupT {
    fn to_rs(self) -> RenderMessage {
        RenderMessage::AddRender(
            self.id,
            self.render_messages
                .into_iter()
                .map(FlatToRs::to_rs)
                .collect(),
        )
    }
}

impl FlatToRs<RenderMessage> for flat::RemoveRenderGroupT {
    fn to_rs(self) -> RenderMessage {
        RenderMessage::RemoveRender(self.id)
    }
}
