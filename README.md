# RLBotServer + RocketSim

- Lets RLBotServer use [RocketSim v3](https://github.com/ZealanL/RocketSim/tree/v3-rust) instead of Rocket League!
  - RocketSim v3 is a native Rust implementation of RocketSim that's 10x faster
- Uses [rlbot_flat](https://github.com/RLBot/rust-interface/tree/master/rlbot_flat) to communicate over TCP with RLBotServer
- Uses a single-threaded tokio runtime with [async-timer](https://crates.io/crates/async-timer) for high-resolution sleeping on Linux/MacOS
- Lockstep allows running games as fast as the bots in a match will allow
- Headless games to run matches without rendering them

## Usage

See command lines options with `cargo r -r -- --help`:

```
Usage: rlbot_sim [OPTIONS]

Options:
      --headless                 Run without rendering the game
      --lockstep                 Run as fast as inputs arrive
      --rlbot-port <RLBOT_PORT>  The port to connect to RLBot on [default: 23233]
  -m, --meshes <MESHES>          Path to the collision meshes [default: ./collision_meshes]
      --debug-mesh-init          Enable debug info when loading collision meshes
  -h, --help                     Print help
  -V, --version                  Print version
```

### Running through cargo

- Run a real-time match with the visualizer: `cargo r -r`
- Run without visualizer: `cargo r -r -- --headless`
- Run without visualizer + faster than real-time: `cargo r -r -- --headless --lockstep`
  - Lockstep waits for inputs from all bots to arrive and then immediately steps and sends out the next `GamePacket`. If the bots are running slower than real-time, _lockstep will also run slower_.
