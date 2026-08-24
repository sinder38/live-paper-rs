mod app;
mod backend;
mod config;
mod egl;
mod gamemode;
mod player;
mod render;

use std::path::PathBuf;

use app::App;
use calloop::{EventLoop, channel};
use calloop_wayland_source::WaylandSource;
use clap::Parser;
use config::Config;
use log::{info, warn};
use smithay_client_toolkit::reexports::client::{Connection, globals::registry_queue_init};

// A generated ffmpeg test pattern, so it runs without video file
const DEFAULT_SOURCE: &str = "av://lavfi:testsrc2=size=1280x720:rate=30";

const APP_NAME: &str = "live-paper";

#[derive(Parser)]
#[command(version, about)]
struct Cli {
    /// Video path or mpv-compatible source (overrides `path` in the config)
    video: Option<String>,

    /// Config file to use
    /// (default: $XDG_CONFIG_HOME/live-paper/config.toml)
    #[arg(short, long, value_name = "PATH")]
    config_path: Option<PathBuf>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Basic logging setup, may change later
    env_logger::init();

    let cli = Cli::parse();
    let config = Config::load(cli.config_path)?;

    // CLI arg over config file, otherwise run built-in default
    let video_path = match cli.video.or_else(|| config.player.path.clone()) {
        Some(a) => a,
        None => {
            warn!("Arugment and config path unavailible - using default");
            DEFAULT_SOURCE.to_owned()
        }
    };

    info!("Using Video: at {}", video_path);

    let conn = Connection::connect_to_env()?;
    let (globals, event_queue) = registry_queue_init(&conn)?;
    let qh = event_queue.handle();

    let mut app = App::new(&globals, &qh, &conn, &video_path, &config)?;

    let mut event_loop: EventLoop<App> = EventLoop::try_new()?;
    let loop_handle = event_loop.handle();

    WaylandSource::new(conn, event_queue).insert(loop_handle.clone())?;

    if config.pausing.on_gamemode {
        // Watch gamemode state
        let (gamemode_tx, gamemode_rx) = channel::channel();
        gamemode::watch(gamemode_tx);
        loop_handle.insert_source(gamemode_rx, |event, _, app| {
            if let channel::Event::Msg(active) = event {
                app.set_gamemode(active);
            }
        })?;
    }

    while !app.exit() {
        event_loop.dispatch(None, &mut app)?;
    }
    Ok(())
}
