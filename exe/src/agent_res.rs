use rlbot_sockets::flat;

struct PlayerMetadata {
    index: u32,
    spawn_id: i32,
    team: u32,
    agent_id: String,
    is_reserved: bool,
}

#[derive(Default)]
pub struct AgentReservation {
    known_players: Vec<PlayerMetadata>,
}

impl AgentReservation {
    pub fn set_players(&mut self, match_settings: &flat::MatchSettingsT) {
        self.known_players.clear();

        let mut index_offset = 0;

        for (i, player) in match_settings.player_configurations.iter().enumerate() {
            match player.variety.player_class_type() {
                flat::PlayerClass::Human => index_offset += 1,
                flat::PlayerClass::RLBot => {
                    let index = i as u32 - index_offset;

                    self.known_players.push(PlayerMetadata {
                        index,
                        spawn_id: player.spawn_id,
                        team: player.team,
                        agent_id: player.agent_id.clone(),
                        is_reserved: false,
                    })
                }
                _ => continue,
            }
        }
    }

    pub fn reserve_player(&mut self, agent_id: &str) -> Option<flat::ControllableTeamInfoT> {
        let player = self
            .known_players
            .iter_mut()
            .find(|p| !p.is_reserved && p.agent_id == agent_id)?;
        player.is_reserved = true;

        let mut controllable_info = flat::ControllableInfoT::default();
        controllable_info.index = player.index;
        controllable_info.spawn_id = player.spawn_id;

        let mut team_controllable_info = flat::ControllableTeamInfoT::default();
        team_controllable_info.team = player.team;
        team_controllable_info.controllables = vec![controllable_info];

        Some(team_controllable_info)
    }
}
