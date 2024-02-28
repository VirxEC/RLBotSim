use rlbot_sockets::flat;
use std::{io::Result as IoResult, process::Command};

pub fn auto_start_bots(match_settings: &flat::MatchSettingsT) -> IoResult<()> {
    if !match_settings.auto_start_bots {
        return Ok(());
    }

    for player in &match_settings.player_configurations {
        dbg!(&player.name);
        dbg!(player.spawn_id);

        let parts = shlex::split(&player.run_command).unwrap();

        let mut command = Command::new(&parts[0]);
        command.env("BOT_SPAWN_ID", &player.spawn_id.to_string());
        command.current_dir(&player.location);
        command.args(&parts[1..]);

        command.spawn()?;
    }

    Ok(())
}
