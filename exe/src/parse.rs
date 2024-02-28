use rlbot_sockets::flat;
use std::{
    collections::HashMap, hash::{DefaultHasher, Hash, Hasher}, io::Result as IoResult, path::Path
};
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

    let mut names = HashMap::with_capacity(cars_len);
    let mut spawn_id_hasher = DefaultHasher::new();

    for car in cars_header[0..cars_len].iter() {
        let mut player = flat::PlayerConfigurationT::default();

        player.variety = flat::PlayerClassT::RLBot(Box::default());

        let player_team = car["team"].as_integer().unwrap_or_default();
        player.team = player_team as u32;

        let Some(relative_config_path) = car.get("config").and_then(|c| c.as_str()) else {
            continue;
        };

        let config_path = path.parent().unwrap().join(relative_config_path);
        let config = fs::read_to_string(&config_path).await?;
        let config_path_parent = config_path.parent().unwrap();
        let config_toml = config.parse::<toml::Table>().unwrap_or(empty_map.clone());

        let settings_header = config_toml["settings"].as_table().unwrap_or(&empty_map);
        let name = settings_header["name"]
            .as_str()
            .unwrap_or_default()
            .to_string();

        // Ensure that the name is unique
        // "name" then "name (2)" then "name (3)" etc.
        let num_others = names.entry(name.clone()).or_insert(0);
        
        player.name = if *num_others == 0 {
            name
        } else {
            format!("{name} ({num_others})")
        };

        *num_others += 1;

        player.name.hash(&mut spawn_id_hasher);
        let full_hash = spawn_id_hasher.finish() as i64;
        let wrapped_hash = full_hash % (i32::MAX as i64);
        player.spawn_id = wrapped_hash as i32;

        let location = settings_header["location"].as_str().unwrap_or_default();
        player.location = config_path_parent.join(location).to_string_lossy().to_string();
        player.run_command = settings_header["run_command"]
            .as_str()
            .unwrap_or_default()
            .parse()
            .unwrap_or_default();

        settings.player_configurations.push(player);
    }

    Ok(settings)
}
