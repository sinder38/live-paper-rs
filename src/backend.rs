use std::ffi::c_void;

use log::debug;

use crate::player::Player;
use crate::render::Renderer;

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
}
