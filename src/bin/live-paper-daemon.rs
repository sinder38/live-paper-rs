use std::path::PathBuf;

use clap::Parser;
use live_paper::config::Config;
use live_paper::{DEFAULT_SOURCE, daemon, wallpaper};
use log::warn;

#[derive(Parser)]
#[command(version, about = "live-paper daemon")]
struct Cli {
    /// Video path or mpv-compatible source (overrides `path` in the config)
    video: Option<String>,

    /// Config file to use
    /// (default: $XDG_CONFIG_HOME/live-paper/config.toml)
    #[arg(short, long, value_name = "PATH")]
    config_path: Option<PathBuf>,

    /// Internal: run as the supervised renderer, reading commands on stdin
    #[arg(long, hide = true)]
    renderer: bool,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Basic logging setup, may change later
    env_logger::init();

    let cli = Cli::parse();

    if cli.renderer {
        let (config, video) = wallpaper::read_init()?;
        return wallpaper::run(&config, &video, true);
    }

    let config = Config::load(cli.config_path.clone())?;

    // CLI arg over config file, otherwise run built-in default
    let video = match cli.video.or_else(|| config.player.path.clone()) {
        Some(video) => video,
        None => {
            warn!("Arugment and config path unavailible - using default");
            DEFAULT_SOURCE.to_owned()
        }
    };

    daemon::run(config, cli.config_path, video)
}
