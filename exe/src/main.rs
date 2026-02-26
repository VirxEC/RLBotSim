use std::path::PathBuf;

use clap::{
    Parser,
    builder::styling::{AnsiColor, Effects, Style},
};
use rocketsim::init;

use crate::game::GameState;

mod connection;
mod conversion;
mod game;

/// The default port that we can use to talk to RLBot
const RLBOT_PORT: u16 = 23233;

pub const HEADER: Style = AnsiColor::BrightGreen.on_default().effects(Effects::BOLD);
pub const USAGE: Style = AnsiColor::BrightGreen.on_default().effects(Effects::BOLD);
pub const LITERAL: Style = AnsiColor::BrightCyan.on_default().effects(Effects::BOLD);
pub const PLACEHOLDER: Style = AnsiColor::Cyan.on_default();

pub const CLAP_STYLING: clap::builder::styling::Styles = clap::builder::styling::Styles::styled()
    .header(HEADER)
    .usage(USAGE)
    .literal(LITERAL)
    .placeholder(PLACEHOLDER);

#[derive(Parser, Debug)]
#[command(version, about, long_about = None, styles=CLAP_STYLING)]
struct CliArgs {
    #[arg(long)]
    /// Runs the simulation without rendering the game
    headless: bool,
    #[arg(long)]
    /// Runs the simulation as fast as inputs arrive
    lockstep: bool,
    #[arg(long, value_parser = clap::value_parser!(u16).range(1..), default_value_t = RLBOT_PORT)]
    /// The port to connect to RLBot on
    rlbot_port: u16,
    #[arg(short, long, default_value = "./collision_meshes")]
    /// Path to the collision meshes
    meshes: PathBuf,
    #[arg(long)]
    /// Enable debug info when loading collision meshes
    debug_mesh_init: bool,
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let args = CliArgs::parse();
    init(&args.meshes, !args.debug_mesh_init).expect("Failed to find collision meshes");

    GameState::new(args).await.run().await;
}
