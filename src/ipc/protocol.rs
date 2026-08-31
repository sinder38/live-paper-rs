use serde::{Deserialize, Serialize};

use crate::config::{BackendKind, Config};

/// A client asking the daemon to do something
#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Request {
    /// Change the wallpaper and/or playback settings. Every field is optional so
    /// `set --speed 2` can tweak one thing without touching the video
    Set {
        video: Option<String>,
        speed: Option<f64>,
        mute: Option<bool>,
        fill: Option<bool>,
        /// Reserved for per-output wallpapers; the daemon rejects it for now
        output: Option<String>,
    },
    Query,
    Pause,
    Resume,
    Toggle,
    /// Re-read the config file from disk
    Reload,
    /// Replace the renderer process, reclaiming what libmpv leaked
    Restart,
    /// Stop the daemon
    Kill,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Response {
    Ok { message: Option<String> },
    State(Box<State>),
    Error { message: String },
}

impl Response {
    pub fn ok() -> Self {
        Response::Ok { message: None }
    }

    pub fn msg(message: impl Into<String>) -> Self {
        Response::Ok {
            message: Some(message.into()),
        }
    }

    pub fn error(message: impl std::fmt::Display) -> Self {
        Response::Error {
            message: message.to_string(),
        }
    }
}

/// What the daemon knows about the running wallpaper. The renderer half is
/// filled in from the child's last [`RenderEvent::Status`], so it is empty until
/// the child has configured its surface
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct State {
    pub video: String,
    pub backend: BackendKind,
    /// Connector name of the output we ended up on, e.g. "DP-1"
    pub output: Option<String>,
    /// Compositor-side surface size
    pub logical: (u32, u32),
    /// Hardware resolution we actually render at
    pub physical: (u32, u32),
    pub paused: bool,
    /// Why playback is paused,
    pub pause_reasons: Vec<String>,
    pub renderer_pid: Option<u32>,
    pub renderer_uptime_secs: u64,
    pub renderer_rss_kb: Option<u64>,
}

/// Daemon telling the renderer child what to do
#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum RenderCmd {
    Init {
        config: Box<Config>,
        video: String,
    },
    SetVideo {
        path: String,
    },
    SetSpeed {
        speed: f64,
    },
    SetMute {
        mute: bool,
    },
    SetFill {
        fill: bool,
    },
    /// Pause requested by the user, on top of the automatic pausing
    SetManualPause {
        paused: bool,
    },
    Quit,
}

/// Renderer child reporting back to the daemon
#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum RenderEvent {
    /// First frame is on screen
    Ready,
    /// Pushed whenever the child's state changes, so `query` never has to ask
    Status(Box<State>),
    Error {
        message: String,
    },
}
