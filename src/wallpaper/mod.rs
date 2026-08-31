use std::io::{BufRead, BufReader, Stdout, Write};

use calloop::{EventLoop, channel};
use calloop_wayland_source::WaylandSource;
use log::{error, info, warn};
use smithay_client_toolkit::reexports::client::{Connection, globals::registry_queue_init};

mod app1;
mod backend;
mod egl;
mod gamemode;

use app1::App;

use crate::config::Config;
use crate::ipc::{RenderCmd, RenderEvent, write_line};

/// Where a renderer sends its status, if anyone is listening
enum Reporter {
    /// Standalone run; nothing to report to
    None,
    /// Supervised run; send JSON to daemon
    Daemon(Stdout),
}

impl Reporter {
    fn send(&mut self, event: &RenderEvent) {
        let Reporter::Daemon(out) = self else {
            return;
        };
        // The daemon going away is not recoverable from here; the pipe closing
        // will take this process down on the next write anyway
        if let Err(e) = write_line(out, event) {
            error!("Failed to report to the daemon: {e}");
        }
    }
}

/// Read the `Init` command the daemon always sends first.
///
/// Blocks, because there is nothing to draw until it arrives.
pub fn read_init() -> Result<(Config, String), Box<dyn std::error::Error>> {
    let mut line = String::new();
    if std::io::stdin().lock().read_line(&mut line)? == 0 {
        return Err("daemon closed stdin before sending the initial config".into());
    }

    match serde_json::from_str::<RenderCmd>(&line)? {
        RenderCmd::Init { config, video } => Ok((*config, video)),
        other => Err(format!("expected an init command from the daemon, got {other:?}").into()),
    }
}

/// Forward JSON lines on stdin into the event loop.
///
/// A thread rather than a calloop source because std cannot put a pipe into
/// non-blocking mode; same shape as the gamemode watcher.
fn watch_stdin(sender: channel::Sender<RenderCmd>) {
    std::thread::spawn(move || {
        let stdin = BufReader::new(std::io::stdin());
        for line in stdin.lines() {
            let line = match line {
                Ok(line) => line,
                Err(e) => {
                    error!("Failed to read from the daemon: {e}");
                    break;
                }
            };

            match serde_json::from_str::<RenderCmd>(&line) {
                // Sending fails once the event loop is gone
                Ok(cmd) => {
                    if sender.send(cmd).is_err() {
                        break;
                    }
                }
                Err(e) => error!("Ignoring unparsable command from the daemon: {e}"),
            }
        }

        // stdin closed: the daemon is gone, so nothing can control this
        // wallpaper any more. Do not outlive it
        info!("Lost the daemon, shutting down");
        let _ = sender.send(RenderCmd::Quit);
    });
}

/// Apply one command from the daemon
fn apply(
    app: &mut App,
    cmd: RenderCmd,
    qh: &smithay_client_toolkit::reexports::client::QueueHandle<App>,
) {
    let applied = match cmd {
        RenderCmd::Init { .. } => {
            warn!("Ignoring a second init command; restart the renderer to change it");
            Ok(())
        }
        RenderCmd::SetVideo { path } => app.set_video(&path),
        RenderCmd::SetSpeed { speed } => app.set_speed(speed),
        RenderCmd::SetMute { mute } => app.set_mute(mute),
        RenderCmd::SetFill { fill } => app.set_fill(fill),
        RenderCmd::SetManualPause { paused } => {
            app.set_manual_pause(paused, qh);
            Ok(())
        }
        RenderCmd::Quit => {
            info!("Shutting down at the daemon's request");
            app.quit();
            Ok(())
        }
    };

    if let Err(e) = applied {
        error!("Failed to apply command: {e}");
    }
}

/// Build the surface and run until the wallpaper is torn down
pub fn run(
    config: &Config,
    video: &str,
    supervised: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    info!("Using Video: at {video}");

    let conn = Connection::connect_to_env()?;
    let (globals, event_queue) = registry_queue_init(&conn)?;
    let qh = event_queue.handle();

    let mut app = App::new(&globals, &qh, &conn, video, config)?;

    let mut event_loop: EventLoop<App> = EventLoop::try_new()?;
    let loop_handle = event_loop.handle();

    WaylandSource::new(conn, event_queue).insert(loop_handle.clone())?;

    if config.pausing.on_gamemode {
        // Watch gamemode state
        let (gamemode_tx, gamemode_rx) = channel::channel();
        gamemode::watch(gamemode_tx);
        let gamemode_qh = qh.clone();
        loop_handle.insert_source(gamemode_rx, move |event, _, app| {
            if let channel::Event::Msg(active) = event {
                app.set_gamemode(active, &gamemode_qh);
            }
        })?;
    }

    let mut reporter = Reporter::None;
    if supervised {
        reporter = Reporter::Daemon(std::io::stdout());

        let (cmd_tx, cmd_rx) = channel::channel();
        watch_stdin(cmd_tx);
        let cmd_qh = qh.clone();
        loop_handle.insert_source(cmd_rx, move |event, _, app| {
            if let channel::Event::Msg(cmd) = event {
                apply(app, cmd, &cmd_qh);
            }
        })?;
    }

    while !app.exit() {
        event_loop.dispatch(None, &mut app)?;

        if app.take_ready() {
            reporter.send(&RenderEvent::Ready);
        }
        if let Some(state) = app.take_status() {
            reporter.send(&RenderEvent::Status(Box::new(state)));
        }
    }

    // Let the daemon see a clean shutdown rather than a truncated pipe
    if let Reporter::Daemon(out) = &mut reporter {
        let _ = out.flush();
    }
    Ok(())
}
