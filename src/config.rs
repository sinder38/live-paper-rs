use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;

use log::{error, info, warn};
use smithay_client_toolkit::shell::wlr_layer::Layer;

use crate::APP_NAME;

/// Config is loaded from `$XDG_CONFIG_HOME/live-paper/config.toml`
///
/// Every field has a default, so an absent or partial file is fine.
#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub struct Config {
    /// Which frame source to use
    pub backend: BackendKind,
    /// Mpv player confiruration
    pub player: PlayerConfig,
    /// Wayland layer configuration
    pub layer: LayerConfig,
    /// Pausing config
    #[serde(alias = "pause")]
    pub pausing: PausingConfig,
    /// Enable debug logging
    pub debug: DebugConfig,
}

#[derive(Debug, Deserialize, Default, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum BackendKind {
    /// Play a video file/stream
    #[default]
    #[serde(alias = "player")]
    Mpv,
    /// Draw the built-in glow pattern
    #[serde(alias = "glow")]
    Pattern,
}

#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "libmpv-restart", derive(Clone))]
#[serde(default)]
pub struct PlayerConfig {
    /// Video path, overridable by the CLI arg
    pub path: Option<String>,
    pub speed: f64,
    pub mute: bool,
    pub hwdec: String,
    /// `true` crops the video to fill the screen
    /// `false` shows the whole video and letterboxes it
    pub fill: bool,
    /// Raw passthrough to `mpv.set_property`, applied after the typed fields!
    /// Full option list: https://mpv.io/manual/master/#options
    pub mpv_options: HashMap<String, String>,
    /// Hours between forced mpv restarts; works around a known upstream leak
    #[cfg(feature = "libmpv-restart")]
    pub mpv_restart_hours: u64,
}

impl Default for PlayerConfig {
    fn default() -> Self {
        Self {
            path: None,
            speed: 1.0,
            mute: true,
            hwdec: "auto".to_string(),
            fill: true,
            mpv_options: HashMap::new(),
            #[cfg(feature = "libmpv-restart")]
            mpv_restart_hours: 1,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct LayerConfig {
    /// "background" | "bottom" | "top" | "overlay"
    pub layer: String,
    pub exclusive_zone: i32,
}

impl Default for LayerConfig {
    fn default() -> Self {
        Self {
            layer: "background".to_string(),
            exclusive_zone: -1,
        }
    }
}

/// Configures automatic pausing for the backend
#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct PausingConfig {
    /// Pause when fullscreening an application (per workspace)
    pub on_fullscreen: bool,
    /// Pause when an application is maximized (not fullscreen)
    pub on_maximized: bool,
    /// When gamemode is on
    pub on_gamemode: bool,
    /// When the screen is off
    pub on_screen_off: bool,
}

impl Default for PausingConfig {
    fn default() -> Self {
        Self {
            on_fullscreen: true,
            on_maximized: true,
            on_gamemode: true,
            on_screen_off: true,
        }
    }
}

// TODO: this doesn't work for the whole application as log init doesn't take it right now
#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub struct DebugConfig {
    pub enabled: bool,
}

impl Config {
    /// Load configuration.
    pub fn load(path: Option<PathBuf>) -> Result<Self, Box<dyn std::error::Error>> {
        match path {
            Some(path) => {
                info!("Using config at: {}", path.display());
                let contents = std::fs::read_to_string(&path)
                    .map_err(|e| format!("reading config {}: {e}", path.display()))?;
                let cfg = toml::from_str(&contents)
                    .map_err(|e| format!("parsing config {}: {e}", path.display()))?;
                Ok(cfg)
            }
            None => {
                warn!("No config provided! Using config defaults");
                Ok(Self::load_default())
            }
        }
    }

    /// Lenient load from the standard XDG path, falling back to defaults
    fn load_default() -> Self {
        let path = config_path();
        match std::fs::read_to_string(&path) {
            Ok(contents) => match toml::from_str(&contents) {
                Ok(cfg) => cfg,
                Err(e) => {
                    error!("Failed to parse config at {}: {}", path.display(), e);
                    Config::default()
                }
            },
            Err(_) => Config::default(),
        }
    }
}

/// Get config path
fn config_path() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join(APP_NAME).join("config.toml")
}

/// Map the `layer.layer` config string onto smithay's `Layer` enum
pub(crate) fn parse_layer(name: &str) -> Layer {
    match name {
        "background" => Layer::Background,
        "bottom" => Layer::Bottom,
        "top" => Layer::Top,
        "overlay" => Layer::Overlay,
        other => {
            warn!("Unknown layer \"{other}\", falling back to \"background\"");
            Layer::Background
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_partial_toml() {
        let cfg: Config = toml::from_str("[player]\npath = \"/x.mp4\"\nspeed = 2.0\n").unwrap();
        assert_eq!(cfg.player.path.as_deref(), Some("/x.mp4"));
        assert_eq!(cfg.player.speed, 2.0);
        assert!(cfg.player.mute); // untouched field keeps its default
    }

    #[test]
    fn backend_defaults_to_mpv() {
        let cfg: Config = toml::from_str("").unwrap();
        assert_eq!(cfg.backend, BackendKind::Mpv);
    }

    #[test]
    fn parses_backend_choice() {
        let cfg: Config = toml::from_str("backend = \"pattern\"\n").unwrap();
        assert_eq!(cfg.backend, BackendKind::Pattern);
    }
}
