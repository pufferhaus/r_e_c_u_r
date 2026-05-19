//! Static config (`config.toml`) and user-state-dir resolution.

use serde::Deserialize;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

/// Resolve the directory where user-writable state lives.
///
/// Order:
///   1. `$RECUR_STATE_DIR` (set by the systemd unit on Pi).
///   2. `<exec_dir>/.config/recur/` next to the binary.
///   3. `./.config/recur/` relative to CWD (last-ditch).
///
/// Always returns a path that exists (created if needed).
pub fn user_state_dir() -> PathBuf {
    if let Some(env_dir) = std::env::var_os("RECUR_STATE_DIR") {
        let p = PathBuf::from(env_dir);
        let _ = std::fs::create_dir_all(&p);
        return p;
    }
    let base = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));
    let dir = base.join(".config").join("recur");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub render: RenderConfig,
    #[serde(default)]
    pub detour: Option<DetourConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RenderConfig {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct DetourConfig {
    #[serde(default)]
    pub ring_budget_mb: Option<u64>,
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let s = std::fs::read_to_string(path)?;
        toml::from_str(&s).map_err(|e| Error::TomlParse {
            file: path.display().to_string(),
            source: e,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_config() {
        let s = r#"
            [render]
            width = 720
            height = 480
            fps = 30
        "#;
        let cfg: Config = toml::from_str(s).unwrap();
        assert_eq!(cfg.render.width, 720);
        assert_eq!(cfg.render.fps, 30);
    }

    #[test]
    fn parses_config_with_detour_section() {
        let s = r#"
            [render]
            width = 720
            height = 480
            fps = 30
            [detour]
            ring_budget_mb = 256
        "#;
        let cfg: Config = toml::from_str(s).unwrap();
        assert_eq!(
            cfg.detour.as_ref().and_then(|d| d.ring_budget_mb),
            Some(256)
        );
    }

    #[test]
    fn parses_config_without_detour_section() {
        let s = r#"
            [render]
            width = 720
            height = 480
            fps = 30
        "#;
        let cfg: Config = toml::from_str(s).unwrap();
        assert!(
            cfg.detour.is_none() || cfg.detour.as_ref().and_then(|d| d.ring_budget_mb).is_none()
        );
    }

    #[test]
    fn user_state_dir_returns_existing_path_with_env() {
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("RECUR_STATE_DIR", tmp.path());
        let got = user_state_dir();
        assert!(got.exists());
        assert_eq!(got, tmp.path());
        std::env::remove_var("RECUR_STATE_DIR");
    }
}
