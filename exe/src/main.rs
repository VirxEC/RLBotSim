use std::path::PathBuf;

use clap::Parser;
use rocketsim::init;

use crate::game::GameState;

mod connection;
mod game;

/// The default port that we can use to talk to RLBot
const RLBOT_PORT: u16 = 23233;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct CliArgs {
    #[arg(long)]
    headless: bool,
    #[arg(long, value_parser = clap::value_parser!(u16).range(1..), default_value_t = RLBOT_PORT)]
    rlbot_port: u16,
    #[arg(long)]
    lockstep: bool,
    #[arg(short, long, default_value = "./collision_meshes")]
    meshes: PathBuf,
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let args = CliArgs::parse();
    init(&args.meshes, true).expect("Failed to find collision meshes");

    GameState::new(args).await.run().await;
}
