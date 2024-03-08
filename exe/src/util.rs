use rlbot_sockets::flat;
use rocketsim_rs::math::{Angle, RotMat, Vec3};
use std::{io::Result as IoResult, process::Command};

pub fn auto_start_bots(match_settings: &flat::MatchSettingsT) -> IoResult<()> {
    if !match_settings.auto_start_bots {
        return Ok(());
    }

    for player in &match_settings.player_configurations {
        let parts = shlex::split(&player.run_command).unwrap();

        let mut command = Command::new(&parts[0]);
        command.env("BOT_SPAWN_ID", &player.spawn_id.to_string());
        command.current_dir(&player.location);
        command.args(&parts[1..]);

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
