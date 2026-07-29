mod app;
mod backend;
mod config;
mod egl;
mod player;
mod render;

use app::App;
use config::Config;
use log::info;
use smithay_client_toolkit::reexports::client::{Connection, globals::registry_queue_init};

// A generated ffmpeg test pattern, so it runs without video file
const DEFAULT_SOURCE: &str = "av://lavfi:testsrc2=size=1280x720:rate=30";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::load();

    // Basic logging setup, may change later
    env_logger::init();

    // CLI args over config file, otherwise run built-in default
    let video_path = std::env::args()
        .nth(1)
        .or_else(|| config.path.clone())
        .unwrap_or_else(|| DEFAULT_SOURCE.to_owned());

    info!("Using Video: at {}", video_path);

    let conn = Connection::connect_to_env()?;
    let (globals, mut event_queue) = registry_queue_init(&conn)?;
    let qh = event_queue.handle();

    let mut app = App::new(&globals, &qh, &conn, &video_path, &config)?;

    while !app.exit() {
        event_queue.blocking_dispatch(&mut app)?;
    }
    Ok(())
}
