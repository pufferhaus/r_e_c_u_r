//! Keymap: loads `keymap.toml` and maps key-code strings → `Action`.
//!
//! The TOML format is a `[bindings]` table where each key is a physical-key
//! name string and each value is an action string:
//!
//! ```toml
//! [bindings]
//! "Space" = "TogglePlayPause"
//! "Digit1" = "SelectSlot(1)"
//! "KeyB"   = "EnterMode(Browser)"
//! ```
//!
//! Action strings are parsed by `parse_action`. Unrecognised strings return
//! `Error::Keymap`.

use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;

use crate::action::{Action, SettingId};
use crate::error::{Error, Result};
use crate::state::DisplayMode;

#[derive(Debug, Deserialize)]
struct KeymapFile {
    bindings: HashMap<String, String>,
}

/// A loaded keymap: maps raw key-code strings to `Action` values.
#[derive(Debug, Default)]
pub struct Keymap {
    map: HashMap<String, Action>,
}

impl Keymap {
    /// Load and parse `keymap.toml` at `path`.
    pub fn load(path: &Path) -> Result<Self> {
        let s = std::fs::read_to_string(path)?;
        Self::parse(&s)
    }

    /// Parse from a TOML string (useful for tests).
    pub fn parse(s: &str) -> Result<Self> {
        let file: KeymapFile = toml::from_str(s).map_err(|e| Error::TomlParse {
            file: "keymap.toml".into(),
            source: e,
        })?;
        let mut map = HashMap::new();
        for (key, action_str) in file.bindings {
            let action = parse_action(&action_str)
                .map_err(|_| Error::Keymap(format!("{key} = {action_str:?}")))?;
            map.insert(key, action);
        }
        Ok(Self { map })
    }

    /// Look up the `Action` for a raw key-code string, e.g. `"Space"`.
    pub fn lookup(&self, key: &str) -> Option<Action> {
        self.map.get(key).cloned()
    }

    /// Iterate all (key_code, action) pairs (useful for diagnostics).
    pub fn entries(&self) -> impl Iterator<Item = (&str, &Action)> {
        self.map.iter().map(|(k, v)| (k.as_str(), v))
    }
}

/// Parse an action label string like `"TogglePlayPause"`, `"SelectSlot(3)"`,
/// `"EnterMode(Browser)"`.
fn parse_action(s: &str) -> std::result::Result<Action, ()> {
    // Parenthesized variants.
    if let Some(rest) = s.strip_prefix("SelectSlot(").and_then(|r| r.strip_suffix(')')) {
        let n: u8 = rest.parse().map_err(|_| ())?;
        return Ok(Action::SelectSlot(n));
    }
    if let Some(rest) = s.strip_prefix("EnterMode(").and_then(|r| r.strip_suffix(')')) {
        let mode = match rest {
            "Browser" => DisplayMode::Browser,
            "Sampler" => DisplayMode::Sampler,
            "Settings" => DisplayMode::Settings,
            "Shaders" => DisplayMode::Shaders,
            "ShdrBnk" => DisplayMode::ShdrBnk,
            "Frames" => DisplayMode::Frames,
            _ => return Err(()),
        };
        return Ok(Action::EnterMode(mode));
    }
    if let Some(rest) = s.strip_prefix("SeekRelative(").and_then(|r| r.strip_suffix(')')) {
        let v: f64 = rest.parse().map_err(|_| ())?;
        return Ok(Action::SeekRelative(v));
    }
    if let Some(rest) = s.strip_prefix("SetRate(").and_then(|r| r.strip_suffix(')')) {
        let v: f32 = rest.parse().map_err(|_| ())?;
        return Ok(Action::SetRate(v));
    }
    if let Some(rest) = s.strip_prefix("SelectShaderSlot(").and_then(|r| r.strip_suffix(')')) {
        let n: u8 = rest.parse().map_err(|_| ())?;
        return Ok(Action::SelectShaderSlot(n));
    }
    if let Some(rest) = s.strip_prefix("TriggerShaderSlot(").and_then(|r| r.strip_suffix(')')) {
        let n: u8 = rest.parse().map_err(|_| ())?;
        return Ok(Action::TriggerShaderSlot(n));
    }
    if let Some(rest) = s.strip_prefix("ShaderParamAdjust(").and_then(|r| r.strip_suffix(')')) {
        let n: i8 = rest.parse().map_err(|_| ())?;
        return Ok(Action::ShaderParamAdjust(n));
    }
    if let Some(rest) = s.strip_prefix("ShaderParamSelect(").and_then(|r| r.strip_suffix(')')) {
        let n: u8 = rest.parse().map_err(|_| ())?;
        return Ok(Action::ShaderParamSelect(n));
    }
    if let Some(rest) = s.strip_prefix("DetourScrubBy(").and_then(|r| r.strip_suffix(')')) {
        let n: i32 = rest.parse().map_err(|_| ())?;
        return Ok(Action::DetourScrubBy(n));
    }
    if let Some(rest) = s.strip_prefix("CycleSetting(").and_then(|r| r.strip_suffix(')')) {
        let id = match rest {
            "LoopType" => SettingId::LoopType,
            "OnFinish" => SettingId::OnFinish,
            "OnStart" => SettingId::OnStart,
            "OnLoad" => SettingId::OnLoad,
            "LoadNext" => SettingId::LoadNext,
            "RandStartMode" => SettingId::RandStartMode,
            "FixedLengthMode" => SettingId::FixedLengthMode,
            "FixedLengthMultiply" => SettingId::FixedLengthMultiply,
            "ResetPlayers" => SettingId::ResetPlayers,
            _ => return Err(()),
        };
        return Ok(Action::CycleSetting(id));
    }

    // Simple variants.
    let action = match s {
        "NavUp" => Action::NavUp,
        "NavDown" => Action::NavDown,
        "NavLeft" => Action::NavLeft,
        "NavRight" => Action::NavRight,
        "Enter" => Action::Enter,
        "Back" => Action::Back,
        "Panic" => Action::Panic,
        "ToggleNowNext" => Action::ToggleNowNext,
        "ToggleFunction" => Action::ToggleFunction,
        "PrevBank" => Action::PrevBank,
        "NextBank" => Action::NextBank,
        "SetLoopIn" => Action::SetLoopIn,
        "SetLoopOut" => Action::SetLoopOut,
        "ClearLoop" => Action::ClearLoop,
        "TogglePlayPause" => Action::TogglePlayPause,
        "Reload" => Action::Reload,
        "DetourEnter" => Action::DetourEnter,
        "DetourExit" => Action::DetourExit,
        "DetourCycleSpeed" => Action::DetourCycleSpeed,
        "DetourToggleDirection" => Action::DetourToggleDirection,
        "DetourTogglePlay" => Action::DetourTogglePlay,
        "DetourSetStartMarker" => Action::DetourSetStartMarker,
        "DetourSetEndMarker" => Action::DetourSetEndMarker,
        "DetourClearMarkers" => Action::DetourClearMarkers,
        "DetourCycleMix" => Action::DetourCycleMix,
        _ => return Err(()),
    };
    Ok(action)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
[bindings]
"Space" = "TogglePlayPause"
"Digit1" = "SelectSlot(1)"
"Digit0" = "SelectSlot(0)"
"KeyB"   = "EnterMode(Browser)"
"Enter"  = "Enter"
"Escape" = "Back"
"ArrowUp" = "NavUp"
"ShiftLeft" = "ToggleFunction"
"#;

    #[test]
    fn load_toggle_play_pause() {
        let km = Keymap::parse(SAMPLE).unwrap();
        assert_eq!(km.lookup("Space"), Some(Action::TogglePlayPause));
    }

    #[test]
    fn select_slot_zero() {
        let km = Keymap::parse(SAMPLE).unwrap();
        assert_eq!(km.lookup("Digit0"), Some(Action::SelectSlot(0)));
    }

    #[test]
    fn select_slot_one() {
        let km = Keymap::parse(SAMPLE).unwrap();
        assert_eq!(km.lookup("Digit1"), Some(Action::SelectSlot(1)));
    }

    #[test]
    fn enter_mode_browser() {
        let km = Keymap::parse(SAMPLE).unwrap();
        assert_eq!(km.lookup("KeyB"), Some(Action::EnterMode(DisplayMode::Browser)));
    }

    #[test]
    fn unknown_key_returns_none() {
        let km = Keymap::parse(SAMPLE).unwrap();
        assert_eq!(km.lookup("Quux"), None);
    }

    #[test]
    fn bad_action_string_returns_err() {
        let bad = "[bindings]\n\"Space\" = \"WarpDrive\"\n";
        assert!(Keymap::parse(bad).is_err());
    }

    #[test]
    fn toggle_function_action() {
        let km = Keymap::parse(SAMPLE).unwrap();
        assert_eq!(km.lookup("ShiftLeft"), Some(Action::ToggleFunction));
    }

    #[test]
    fn parses_select_shader_slot() {
        let s = "[bindings]\n\"F1\" = \"SelectShaderSlot(3)\"\n";
        let km = Keymap::parse(s).unwrap();
        assert_eq!(km.lookup("F1"), Some(Action::SelectShaderSlot(3)));
    }

    #[test]
    fn parses_trigger_shader_slot() {
        let s = "[bindings]\n\"F2\" = \"TriggerShaderSlot(7)\"\n";
        let km = Keymap::parse(s).unwrap();
        assert_eq!(km.lookup("F2"), Some(Action::TriggerShaderSlot(7)));
    }

    #[test]
    fn parses_shader_param_adjust() {
        let s = "[bindings]\n\"KeyP\" = \"ShaderParamAdjust(1)\"\n";
        let km = Keymap::parse(s).unwrap();
        assert_eq!(km.lookup("KeyP"), Some(Action::ShaderParamAdjust(1)));
    }

    #[test]
    fn parses_enter_mode_shdr_bnk() {
        let s = "[bindings]\n\"KeyK\" = \"EnterMode(ShdrBnk)\"\n";
        let km = Keymap::parse(s).unwrap();
        assert_eq!(km.lookup("KeyK"), Some(Action::EnterMode(DisplayMode::ShdrBnk)));
    }

    #[test]
    fn parses_all_detour_actions() {
        let s = r#"
            [bindings]
            "KeyD" = "DetourEnter"
            "Escape" = "DetourExit"
            "ArrowLeft" = "DetourScrubBy(-1)"
            "ArrowRight" = "DetourScrubBy(10)"
            "ArrowUp" = "DetourCycleSpeed"
            "ArrowDown" = "DetourToggleDirection"
            "Space" = "DetourTogglePlay"
            "BracketLeft" = "DetourSetStartMarker"
            "BracketRight" = "DetourSetEndMarker"
            "Backslash" = "DetourClearMarkers"
            "KeyM" = "DetourCycleMix"
        "#;
        let km = Keymap::parse(s).unwrap();
        assert_eq!(km.lookup("KeyD"), Some(Action::DetourEnter));
        assert_eq!(km.lookup("Escape"), Some(Action::DetourExit));
        assert_eq!(km.lookup("ArrowLeft"), Some(Action::DetourScrubBy(-1)));
        assert_eq!(km.lookup("ArrowRight"), Some(Action::DetourScrubBy(10)));
        assert_eq!(km.lookup("ArrowUp"), Some(Action::DetourCycleSpeed));
        assert_eq!(km.lookup("ArrowDown"), Some(Action::DetourToggleDirection));
        assert_eq!(km.lookup("Space"), Some(Action::DetourTogglePlay));
        assert_eq!(km.lookup("BracketLeft"), Some(Action::DetourSetStartMarker));
        assert_eq!(km.lookup("BracketRight"), Some(Action::DetourSetEndMarker));
        assert_eq!(km.lookup("Backslash"), Some(Action::DetourClearMarkers));
        assert_eq!(km.lookup("KeyM"), Some(Action::DetourCycleMix));
    }

    #[test]
    fn default_keymap_toml_parses_fully() {
        // Verify that every entry in the shipped keymap.toml is valid.
        let km = Keymap::parse(include_str!("../../keymap.toml")).unwrap();
        // Spot-check a few representative bindings.
        assert_eq!(km.lookup("Space"), Some(Action::TogglePlayPause));
        assert_eq!(km.lookup("Digit0"), Some(Action::SelectSlot(0)));
        assert_eq!(km.lookup("Digit9"), Some(Action::SelectSlot(9)));
        assert_eq!(km.lookup("BracketLeft"), Some(Action::SetLoopIn));
        assert_eq!(km.lookup("BracketRight"), Some(Action::SetLoopOut));
        assert_eq!(km.lookup("Backslash"), Some(Action::ClearLoop));
        assert_eq!(km.lookup("ArrowUp"), Some(Action::NavUp));
        assert_eq!(km.lookup("KeyB"), Some(Action::EnterMode(DisplayMode::Browser)));
        assert_eq!(km.lookup("Escape"), Some(Action::Back));
        assert_eq!(km.lookup("ShiftLeft"), Some(Action::ToggleFunction));
        assert_eq!(km.lookup("KeyH"), Some(Action::EnterMode(DisplayMode::Shaders)));
        assert_eq!(km.lookup("KeyK"), Some(Action::EnterMode(DisplayMode::ShdrBnk)));
        assert_eq!(km.lookup("F1"), Some(Action::TriggerShaderSlot(0)));
        assert_eq!(km.lookup("F10"), Some(Action::TriggerShaderSlot(9)));
    }
}
