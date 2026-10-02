pub mod cli;
pub mod config;
pub mod daemon;
pub mod ipc;
pub mod wallpaper;

// A generated ffmpeg test pattern, so it runs without video file
pub const DEFAULT_SOURCE: &str = "av://lavfi:testsrc2=size=1280x720:rate=30";

pub const APP_NAME: &str = "live-paper";
