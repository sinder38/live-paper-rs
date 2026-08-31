use std::ffi::c_void;

use log::{debug, warn};

pub mod pattern;
pub mod player;

use pattern::Renderer;
use player::Player;

/// Everything a backend needs the first time the GL context exists
pub struct BackendCtx<'a> {
    /// The glow GL function table
    pub gl: &'a glow::Context,
    /// Raw `wl_display` pointer
    pub display_ptr: *mut c_void,
}

// TODO: move into features
/// The active frame source, chosen once at startup from `config.backend`
pub enum Backend {
    /// Play a video file/stream with mpv.
    Mpv(Player),
    /// Draw the built-in glow test pattern.
    Glow(Renderer),
}

impl Backend {
    pub fn init(&mut self, ctx: BackendCtx) -> Result<(), Box<dyn std::error::Error>> {
        match self {
            Backend::Mpv(p) => p.init(ctx),
            Backend::Glow(r) => r.init(ctx),
        }
    }

    /// Draw one frame
    pub fn render(&mut self, gl: &glow::Context, width: i32, height: i32, time: u32) {
        match self {
            Backend::Mpv(p) => p.render(gl, width, height, time),
            Backend::Glow(r) => r.render(gl, width, height, time),
        }
    }

    // TODO: add per monitor/workspace logging
    pub fn pause(&mut self) {
        debug!("Backend paused");
        match self {
            Backend::Mpv(p) => p.pause(),
            Backend::Glow(r) => r.pause(),
        }
    }

    pub fn resume(&mut self) {
        debug!("Backend resumed");
        match self {
            Backend::Mpv(p) => p.resume(),
            Backend::Glow(r) => r.resume(),
        }
    }

    /// Path/URL currently playing, if the backend has one
    pub fn video(&self) -> Option<&str> {
        match self {
            Backend::Mpv(p) => Some(p.path()),
            Backend::Glow(_) => None,
        }
    }

    pub fn set_video(&mut self, path: &str) -> Result<(), Box<dyn std::error::Error>> {
        match self {
            Backend::Mpv(p) => p.set_video(path),
            Backend::Glow(_) => Err(unsupported("video")),
        }
    }

    pub fn set_speed(&mut self, speed: f64) -> Result<(), Box<dyn std::error::Error>> {
        match self {
            Backend::Mpv(p) => p.set_speed(speed),
            Backend::Glow(_) => Err(unsupported("speed")),
        }
    }

    pub fn set_mute(&mut self, mute: bool) -> Result<(), Box<dyn std::error::Error>> {
        match self {
            Backend::Mpv(p) => p.set_mute(mute),
            Backend::Glow(_) => Err(unsupported("mute")),
        }
    }

    pub fn set_fill(&mut self, fill: bool) -> Result<(), Box<dyn std::error::Error>> {
        match self {
            Backend::Mpv(p) => p.set_fill(fill),
            Backend::Glow(_) => Err(unsupported("fill")),
        }
    }
}

fn unsupported(what: &str) -> Box<dyn std::error::Error> {
    warn!("The pattern backend has no {what}; switch `backend` to \"mpv\"");
    format!("the pattern backend has no {what}").into()
}
