use std::path::PathBuf;

use clap::{Parser, Subcommand};

use crate::ipc::{Request, Response, State};

#[derive(Parser)]
#[command(version, about)]
#[command(args_conflicts_with_subcommands = true)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    /// Video path or mpv-compatible source (overrides `path` in the config)
    pub video: Option<String>,

    /// Config file to use
    /// (default: $XDG_CONFIG_HOME/live-paper/config.toml)
    #[arg(short, long, value_name = "PATH", global = true)]
    pub config_path: Option<PathBuf>,
}

#[derive(Subcommand)]
pub enum Command {
    /// Change the wallpaper or its playback settings
    Set {
        /// Video path or mpv-compatible source
        video: Option<String>,
        /// Playback speed, like: 1.5 for 50% faster
        #[arg(long)]
        speed: Option<f64>,
        /// Silence the video
        #[arg(long)]
        mute: Option<bool>,
        /// Crop to fill the screen instead of letterboxing
        #[arg(long)]
        fill: Option<bool>,
        /// Reserved for per-output wallpapers; not supported yet
        #[arg(short, long)]
        output: Option<String>,
    },
    /// Show what the daemon is playing
    Query {
        /// Print raw JSON instead of a summary
        #[arg(long)]
        json: bool,
    },
    /// Hold playback until `resume`
    Pause,
    /// Undo `pause`
    Resume,
    /// Pause if playing, resume if paused
    Toggle,
    /// Re-read the config file
    Reload,
    /// Replace the renderer process, reclaiming leaked memory
    Restart,
    /// Stop the daemon
    Kill,
}

impl Command {
    /// The wire request for this subcommand, or `None` if it needs no daemon
    pub fn request(&self) -> Request {
        match self {
            Command::Set {
                video,
                speed,
                mute,
                fill,
                output,
            } => Request::Set {
                video: video.clone(),
                speed: *speed,
                mute: *mute,
                fill: *fill,
                output: output.clone(),
            },
            Command::Query { .. } => Request::Query,
            Command::Pause => Request::Pause,
            Command::Resume => Request::Resume,
            Command::Toggle => Request::Toggle,
            Command::Reload => Request::Reload,
            Command::Restart => Request::Restart,
            Command::Kill => Request::Kill,
        }
    }
}

/// Print response
/// Returns false if it was an error
pub fn report(response: &Response, json: bool) -> bool {
    // normal prints on purpose
    match response {
        Response::Ok { message } => {
            if let Some(message) = message {
                println!("{message}");
            }
            true
        }
        Response::State(state) => {
            if json {
                match serde_json::to_string_pretty(state) {
                    Ok(text) => eprintln!("{text}"),
                    Err(e) => eprintln!("failed to format the state: {e}"),
                }
            } else {
                print_state(state);
            }
            true
        }
        Response::Error { message } => {
            eprintln!("error: {message}");
            false
        }
    }
}

fn print_state(state: &State) {
    println!("video:    {}", state.video);
    println!("backend:  {:?}", state.backend);
    println!("output:   {}", state.output.as_deref().unwrap_or("unknown"));
    println!(
        "size:     {}x{} logical, {}x{} physical",
        state.logical.0, state.logical.1, state.physical.0, state.physical.1
    );

    if state.paused {
        println!("paused:   yes ({})", state.pause_reasons.join(", "));
    } else {
        println!("paused:   no");
    }

    match state.renderer_pid {
        Some(pid) => println!(
            "renderer: pid {pid}, up {}",
            format_uptime(state.renderer_uptime_secs)
        ),
        None => println!("renderer: not running"),
    }
    if let Some(kb) = state.renderer_rss_kb {
        println!("memory:   {} MiB", kb / 1024);
    }
}

fn format_uptime(secs: u64) -> String {
    let (h, m, s) = (secs / 3600, (secs % 3600) / 60, secs % 60);
    if h > 0 {
        format!("{h}h {m}m")
    } else if m > 0 {
        format!("{m}m {s}s")
    } else {
        format!("{s}s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn bare_video_has_no_subcommand() {
        let cli = Cli::parse_from(["live-paper", "/x.mp4"]);
        assert!(cli.command.is_none());
        assert_eq!(cli.video.as_deref(), Some("/x.mp4"));
    }

    #[test]
    fn set_builds_a_request() {
        let cli = Cli::parse_from(["live-paper", "set", "/x.mp4", "--speed", "2"]);
        let request = cli.command.expect("set is a subcommand").request();
        assert_eq!(
            request,
            Request::Set {
                video: Some("/x.mp4".into()),
                speed: Some(2.0),
                mute: None,
                fill: None,
                output: None,
            }
        );
    }

    #[test]
    fn formats_uptime() {
        assert_eq!(format_uptime(45), "45s");
        assert_eq!(format_uptime(125), "2m 5s");
        assert_eq!(format_uptime(7300), "2h 1m");
    }
}
