use rlbot_core_types::flat;
use std::{io::Result as IoResult, path::Path};
use tokio::fs;
use toml::map::Map;

pub async fn parse_file_for_match_settings(path: String) -> IoResult<flat::MatchSettingsT> {
    let empty_map = Map::new();
    let empty_vec = Vec::new();

    println!("Match settings path: {path}");
    let path = Path::new(&path);
    let file_str = fs::read_to_string(path).await?;
    let toml = file_str.parse::<toml::Table>().unwrap_or(empty_map.clone());

    let mut settings = flat::MatchSettingsT::default();

    let rlbot_header = toml["rlbot"].as_table().unwrap_or(&empty_map);

    settings.auto_start_bots = rlbot_header["auto_start_bots"].as_bool().unwrap_or_default();

    let match_header = toml["match"].as_table().unwrap_or(&empty_map);

    let num_cars = match_header["num_cars"].as_integer().unwrap_or_default() as usize;

    settings.game_mode = match_header["game_mode"].as_str().unwrap().parse().unwrap_or_default();
    settings.instant_start = match_header["start_without_countdown"].as_bool().unwrap_or_default();

    let cars_header = toml["cars"].as_array().unwrap_or(&empty_vec);

    let cars_len = num_cars.min(cars_header.len());
    settings.player_configurations.reserve(cars_len);
    for car in cars_header[0..cars_len].iter() {
        let mut player = flat::PlayerConfigurationT::default();

        let player_team = car["team"].as_integer().unwrap_or_default();

        player.team = player_team as u32;

        let Some(relative_config_path) = car.get("config").and_then(|c| c.as_str()) else {
            continue;
        };

        let config_path = path.parent().unwrap().join(relative_config_path);
        dbg!(config_path.display());
        let config = fs::read_to_string(config_path).await?;
        let config_toml = config.parse::<toml::Table>().unwrap_or(empty_map.clone());

        let settings_header = config_toml["settings"].as_table().unwrap_or(&empty_map);
        player.name = settings_header["name"]
            .as_str()
            .unwrap_or_default()
            .parse()
            .unwrap_or_default();
        player.location = settings_header["location"]
            .as_str()
            .unwrap_or_default()
            .parse()
            .unwrap_or_default();
        player.run_command = settings_header["run_command"]
            .as_str()
            .unwrap_or_default()
            .parse()
            .unwrap_or_default();

        settings.player_configurations.push(player);
    }

    Ok(settings)
}
