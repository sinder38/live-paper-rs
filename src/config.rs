use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;

/// Config is loaded from `$XDG_CONFIG_HOME/live-paper/config.toml`
///
/// Every field has a default, so an absent or partial file is fine.
#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub struct Config {
    /// Video path, overridable by the CLI arg (CLI wins)
    pub path: Option<String>,
    /// Mpv player confiruration
    pub player: PlayerConfig,
    /// Wayland layer configuration
    pub layer: LayerConfig,
    /// Enable debug logging
    pub debug: DebugConfig,
}

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct PlayerConfig {
    pub speed: f64,
    pub mute: bool,
    pub hwdec: String,
    /// Raw passthrough to `mpv.set_property`, applied after the typed fields!
    /// Full option list: https://mpv.io/manual/master/#options
    pub mpv_options: HashMap<String, String>,
}

impl Default for PlayerConfig {
    fn default() -> Self {
        Self {
            speed: 1.0,
            mute: true,
            hwdec: "auto".to_string(),
            mpv_options: HashMap::new(),
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

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub struct DebugConfig {
    pub enabled: bool,
}

impl Config {
    /// Load from the standard config path, falling back to defaults if the
    /// file is missing.
    pub fn load() -> Self {
        let path = config_path();
        match std::fs::read_to_string(&path) {
            Ok(contents) => match toml::from_str(&contents) {
                Ok(cfg) => cfg,
                Err(e) => {
                    eprintln!("Failed to parse config at {}: {}", path.display(), e);
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
    base.join("live-paper").join("config.toml")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_partial_toml() {
        let cfg: Config = toml::from_str("path = \"/x.mp4\"\n[player]\nspeed = 2.0\n").unwrap();
        assert_eq!(cfg.path.as_deref(), Some("/x.mp4"));
        assert_eq!(cfg.player.speed, 2.0);
        assert!(cfg.player.mute); // untouched field keeps its default
    }
}
