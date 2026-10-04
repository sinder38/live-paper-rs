use std::process::ExitCode;

use clap::Parser;
use live_paper::cli::{Cli, Command, report};
use live_paper::config::Config;
use live_paper::{DEFAULT_SOURCE, daemon, ipc, wallpaper};
use log::warn;

fn main() -> ExitCode {
    // Basic logging setup, may change later
    env_logger::init();

    let cli = Cli::parse();
    let result = match &cli.command {
        Some(command) => send(command),
        None => start(&cli),
    };

    match result {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(e) => {
            log::error!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Ask daemon, print what it says
fn send(command: &Command) -> Result<bool, Box<dyn std::error::Error>> {
    let json = matches!(command, Command::Query { json: true });
    let response = ipc::request(&command.request())?;
    Ok(report(&response, json))
}

/// Run the wallpaper: as renderer, as daemon, or all-in-one without daemon
fn start(cli: &Cli) -> Result<bool, Box<dyn std::error::Error>> {
    if cli.renderer {
        let (config, video) = wallpaper::read_init()?;
        wallpaper::run(&config, &video, true)?;
        return Ok(true);
    }

    let config = Config::load(cli.config_path.clone())?;

    // CLI arg over config file, otherwise run built-in default
    let video = match cli.video.clone().or_else(|| config.player.path.clone()) {
        Some(video) => video,
        None => {
            warn!("Arugment and config path unavailible - using default");
            DEFAULT_SOURCE.to_owned()
        }
    };

    if cli.daemonless || !config.daemon {
        log::info!("Running without a daemon");
        wallpaper::run(&config, &video, false)?;
    } else {
        daemon::run(config, cli.config_path.clone(), video)?;
    }
    Ok(true)
}
