use rlbot_sockets::flat;
use std::{
    collections::HashMap,
    hash::{DefaultHasher, Hash, Hasher},
    io::Result as IoResult,
    path::Path,
};
use tokio::fs;
use toml::{map::Map, Value};

pub async fn file_to_match_settings(path: String) -> IoResult<flat::MatchConfigurationT> {
    let empty_map = Map::new();
    let empty_vec = Vec::new();

    let path = Path::new(&path);
    let file_str = fs::read_to_string(path).await?;
    let toml = file_str
        .parse::<toml::Table>()
        .unwrap_or_else(|_| empty_map.clone());

    let mut settings = flat::MatchConfigurationT::default();

    let rlbot_header = toml
        .get("rlbot")
        .and_then(Value::as_table)
        .unwrap_or(&empty_map);

    settings.auto_start_bots = rlbot_header
        .get("auto_start_bots")
        .and_then(Value::as_bool)
        .unwrap_or(true);

    let match_header = toml["match"].as_table().unwrap_or(&empty_map);

    settings.game_mode = match_header["game_mode"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap_or_default();
    settings.instant_start = match_header
        .get("start_without_countdown")
        .and_then(toml::Value::as_bool)
        .unwrap_or_default();

    let cars_header = toml
        .get("cars")
        .and_then(Value::as_array)
        .unwrap_or(&empty_vec);

    settings.player_configurations.reserve(cars_header.len());

    let mut names = HashMap::with_capacity(cars_header.len());
    let mut spawn_id_hasher = DefaultHasher::new();

    for car in &cars_header[0..cars_header.len()] {
        let mut player = flat::PlayerConfigurationT::default();

        player.variety = flat::PlayerClassT::CustomBot(Box::default());

        let player_team = car["team"].as_integer().unwrap_or_default();
        player.team = player_team as u32;

        let Some(relative_config_path) = car.get("config").and_then(|c| c.as_str()) else {
            continue;
        };

        let config_path = path.parent().unwrap().join(relative_config_path);
        let Ok(config) = fs::read_to_string(&config_path).await else {
            eprintln!("Failed to read bot config file at {config_path:?}");
            continue;
        };

        let config_path_parent = config_path.parent().unwrap();
        let config_toml = config
            .parse::<toml::Table>()
            .unwrap_or_else(|_| empty_map.clone());

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

        let root_dir = settings_header.get("root_dir").and_then(Value::as_str).unwrap_or_default();
        player.root_dir = config_path_parent
            .join(root_dir)
            .to_string_lossy()
            .to_string();

        player.run_command = if cfg!(windows) {
            &settings_header["run_command"]
        } else {
            settings_header
                .get("run_command_linux")
                .unwrap_or_else(|| &settings_header["run_command"])
        }
        .as_str()
        .unwrap_or_default()
        .parse()
        .unwrap_or_default();

        player.agent_id = settings_header["agent_id"]
            .as_str()
            .unwrap_or_default()
            .to_string();

        settings.player_configurations.push(player);
    }

    Ok(settings)
}
