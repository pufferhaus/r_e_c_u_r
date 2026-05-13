//! Core state types.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Slot {
    pub location: PathBuf,
    pub name: String,
    /// Loop in (seconds). `-1.0` = unset → defaults to 0.0 at load.
    #[serde(default = "default_unset")]
    pub start: f64,
    /// Loop out (seconds). `-1.0` = unset → defaults to file duration at load.
    #[serde(default = "default_unset")]
    pub end: f64,
    /// Cached file duration. `0.0` if not yet probed.
    #[serde(default)]
    pub length: f64,
    #[serde(default = "default_rate")]
    pub rate: f32,
}

fn default_unset() -> f64 {
    -1.0
}
fn default_rate() -> f32 {
    1.0
}

pub const SLOTS_PER_BANK: usize = 10;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Bank {
    #[serde(default)]
    pub slots: Vec<Option<Slot>>,
}

impl Bank {
    pub fn empty() -> Self {
        Self {
            slots: (0..SLOTS_PER_BANK).map(|_| None).collect(),
        }
    }

    /// Return the first empty slot index, or `None` if full.
    pub fn first_empty(&self) -> Option<usize> {
        self.slots.iter().position(Option::is_none)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoopType {
    Sequential,
    Parallel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OnFinish {
    Switch,
    Repeat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OnStart {
    Play,
    Show,
    PlayShow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OnLoad {
    Show,
    Hide,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoadNext {
    Auto,
    Manual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerMode {
    Now,
    Next,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayMode {
    Browser,
    Sampler,
    Settings,
    Shaders, // stubbed in Phase 1
    ShdrBnk, // stubbed in Phase 1
    Frames,  // stubbed in Phase 1
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlMode {
    Default,
    ShaderParam, // Phase 2
    DetourScrub, // Phase 3
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SamplerSettings {
    pub loop_type: LoopType,
    pub on_finish: OnFinish,
    pub on_start: OnStart,
    pub on_load: OnLoad,
    pub load_next: LoadNext,
    #[serde(default)]
    pub rand_start_mode: bool,
    #[serde(default)]
    pub fixed_length_mode: bool,
    #[serde(default)]
    pub fixed_length: f64,
    #[serde(default = "default_one")]
    pub fixed_length_multiply: f32,
    #[serde(default)]
    pub reset_players: bool,
}

fn default_one() -> f32 {
    1.0
}

impl Default for SamplerSettings {
    fn default() -> Self {
        Self {
            loop_type: LoopType::Sequential,
            on_finish: OnFinish::Switch,
            on_start: OnStart::Play,
            on_load: OnLoad::Show,
            load_next: LoadNext::Auto,
            rand_start_mode: false,
            fixed_length_mode: false,
            fixed_length: 0.0,
            fixed_length_multiply: 1.0,
            reset_players: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SharedState {
    pub banks: Vec<Bank>,
    pub bank_number: u8,
    pub player_mode: PlayerMode,
    pub display_mode: DisplayMode,
    pub control_mode: ControlMode,
    pub function_on: bool,
    pub feedback_active: bool,
    pub sampler: SamplerSettings,
    pub paths_to_browser: Vec<PathBuf>,
}

impl SharedState {
    pub fn new() -> Self {
        Self {
            banks: vec![Bank::empty()],
            bank_number: 0,
            player_mode: PlayerMode::Now,
            display_mode: DisplayMode::Sampler,
            control_mode: ControlMode::Default,
            function_on: false,
            feedback_active: false,
            sampler: SamplerSettings::default(),
            paths_to_browser: Vec::new(),
        }
    }

    pub fn current_bank(&self) -> &Bank {
        &self.banks[self.bank_number as usize]
    }

    pub fn current_bank_mut(&mut self) -> &mut Bank {
        let n = self.bank_number as usize;
        &mut self.banks[n]
    }
}

impl Default for SharedState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn new_bank_has_ten_empty_slots() {
        let b = Bank::empty();
        assert_eq!(b.slots.len(), 10);
        assert!(b.slots.iter().all(Option::is_none));
    }

    #[test]
    fn first_empty_returns_zero_on_empty_bank() {
        assert_eq!(Bank::empty().first_empty(), Some(0));
    }

    #[test]
    fn first_empty_skips_filled_slots() {
        let mut b = Bank::empty();
        b.slots[0] = Some(Slot {
            location: "/tmp/a.mp4".into(),
            name: "a.mp4".into(),
            start: -1.0,
            end: -1.0,
            length: 0.0,
            rate: 1.0,
        });
        assert_eq!(b.first_empty(), Some(1));
    }

    #[test]
    fn first_empty_returns_none_when_full() {
        let mut b = Bank::empty();
        for i in 0..10 {
            b.slots[i] = Some(Slot {
                location: format!("/tmp/{}.mp4", i).into(),
                name: format!("{}.mp4", i),
                start: -1.0,
                end: -1.0,
                length: 0.0,
                rate: 1.0,
            });
        }
        assert_eq!(b.first_empty(), None);
    }

    #[test]
    fn default_sampler_settings_are_sequential_switch() {
        let s = SamplerSettings::default();
        assert_eq!(s.loop_type, LoopType::Sequential);
        assert_eq!(s.on_finish, OnFinish::Switch);
        assert_eq!(s.fixed_length_multiply, 1.0);
    }

    #[test]
    fn shared_state_starts_in_sampler_mode() {
        let s = SharedState::new();
        assert_eq!(s.display_mode, DisplayMode::Sampler);
        assert_eq!(s.bank_number, 0);
        assert_eq!(s.banks.len(), 1);
        assert!(!s.function_on);
    }
}
