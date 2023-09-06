use crate::generated::rlbot_generated::rlbot::flat;

// pub struct LoadoutPaint {
//     pub car_paint_id: i32,
//     pub decal_paint_id: i32,
//     pub wheels_paint_id: i32,
//     pub boost_paint_id: i32,
//     pub antenna_paint_id: i32,
//     pub hat_paint_id: i32,
//     pub trails_paint_id: i32,
//     pub goal_explosion_paint_id: i32,
// }

// impl From<flat::LoadoutPaint<'_>> for LoadoutPaint {
//     #[inline]
//     fn from(value: flat::LoadoutPaint) -> Self {
//         Self {
//             car_paint_id: value.carPaintId(),
//             decal_paint_id: value.decalPaintId(),
//             wheels_paint_id: value.wheelsPaintId(),
//             boost_paint_id: value.boostPaintId(),
//             antenna_paint_id: value.antennaPaintId(),
//             hat_paint_id: value.hatPaintId(),
//             trails_paint_id: value.trailsPaintId(),
//             goal_explosion_paint_id: value.goalExplosionPaintId(),
//         }
//     }
// }

// #[derive(Default)]
// pub struct Color {
//     pub a: u8,
//     pub r: u8,
//     pub g: u8,
//     pub b: u8,
// }

// impl From<flat::Color<'_>> for Color {
//     #[inline]
//     fn from(value: flat::Color) -> Self {
//         Self {
//             a: value.a(),
//             r: value.r(),
//             g: value.g(),
//             b: value.b(),
//         }
//     }
// }

// pub struct PlayerLoadout {
//     pub team_color_id: i32,
//     pub custom_color_id: i32,
//     pub car_id: i32,
//     pub decal_id: i32,
//     pub wheels_id: i32,
//     pub boost_id: i32,
//     pub antenna_id: i32,
//     pub hat_id: i32,
//     pub paint_finish_id: i32,
//     pub custom_finish_id: i32,
//     pub engine_audio_id: i32,
//     pub trails_id: i32,
//     pub goal_explosion_id: i32,
//     pub loadout_paint: LoadoutPaint,
//     pub primary_color_lookup: Option<Color>,
//     pub secondary_color_lookup: Option<Color>,
// }

// impl From<flat::PlayerLoadout<'_>> for PlayerLoadout {
//     #[inline]
//     fn from(value: flat::PlayerLoadout) -> Self {
//         Self {
//             team_color_id: value.teamColorId(),
//             custom_color_id: value.customColorId(),
//             car_id: value.carId(),
//             decal_id: value.decalId(),
//             wheels_id: value.wheelsId(),
//             boost_id: value.boostId(),
//             antenna_id: value.antennaId(),
//             hat_id: value.hatId(),
//             paint_finish_id: value.paintFinishId(),
//             custom_finish_id: value.customFinishId(),
//             engine_audio_id: value.engineAudioId(),
//             trails_id: value.trailsId(),
//             goal_explosion_id: value.goalExplosionId(),
//             loadout_paint: value.loadoutPaint().unwrap().into(),
//             primary_color_lookup: value.primaryColorLookup().map(Into::into),
//             secondary_color_lookup: value.secondaryColorLookup().map(Into::into),
//         }
//     }
// }

pub struct PlayerConfiguration {
    pub name: String,
    // pub loadout: PlayerLoadout,
    pub spawn_id: i32,
}

impl From<flat::PlayerConfiguration<'_>> for PlayerConfiguration {
    #[inline]
    fn from(value: flat::PlayerConfiguration) -> Self {
        Self {
            name: value.name().unwrap().to_string(),
            // loadout: value.loadout().unwrap().into(),
            spawn_id: value.spawnId(),
        }
    }
}

pub struct ExtraInfo {
    pub car_info: Vec<PlayerConfiguration>,
}

impl ExtraInfo {
    pub const fn const_default() -> Self {
        Self { car_info: Vec::new() }
    }
}
