use std::process::ExitCode;

use clap::Parser;
use live_paper::cli::{Cli, Command, report};
use live_paper::config::Config;
use live_paper::{DEFAULT_SOURCE, ipc, wallpaper};
use log::warn;

fn main() -> ExitCode {
    // Basic logging setup, may change later
    env_logger::init();

    log::info!("Running as a cli without a daemon");
    let cli = Cli::parse();

    // Send command to daemon or run wallpaper itself
    let result = match &cli.command {
        Some(command) => send(command),
        None => foreground(&cli),
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

/// All-in-one run: no socket, no daemon, exits when the wallpaper does
fn foreground(cli: &Cli) -> Result<bool, Box<dyn std::error::Error>> {
    let config = Config::load(cli.config_path.clone())?;

    // CLI arg over config file, otherwise run built-in default
    let video = match cli.video.clone().or_else(|| config.player.path.clone()) {
        Some(video) => video,
        None => {
            warn!("Arugment and config path unavailible - using default");
            DEFAULT_SOURCE.to_owned()
        }
    };

    wallpaper::run(&config, &video, false)?;
    Ok(true)
}
