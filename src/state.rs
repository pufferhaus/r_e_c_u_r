//! Core state types.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::capture::CaptureDevice;
use crate::detour::DetourSettings;
use crate::render::shader_assembly::GlesProfile;
use crate::shader::ShaderBank;
use crate::video::{ProbeCache, ProbeRequest};

/// Which media source a slot refers to.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum SourceKind {
    File(PathBuf),
    Capture(CaptureDevice),
}

/// A single clip/source slot in a bank.
// Keep Serialize derived (writes new tagged form); Deserialize is custom for
// back-compat with the legacy `location = "..."` wire form.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Slot {
    pub source: SourceKind,
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

impl Slot {
    /// Returns the file path for a `File`-kind slot, `None` for Capture slots.
    pub fn file_path(&self) -> Option<&std::path::Path> {
        match &self.source {
            SourceKind::File(p) => Some(p.as_path()),
            SourceKind::Capture(_) => None,
        }
    }
}

// Custom Deserialize: accepts both the old (`location = "..."`) and the new
// (`source = { kind = "file", value = "..." }`) wire forms.
impl<'de> serde::Deserialize<'de> for Slot {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        struct Wire {
            #[serde(default)]
            location: Option<PathBuf>,
            #[serde(default)]
            source: Option<SourceKind>,
            name: String,
            #[serde(default = "default_unset")]
            start: f64,
            #[serde(default = "default_unset")]
            end: f64,
            #[serde(default)]
            length: f64,
            #[serde(default = "default_rate")]
            rate: f32,
        }
        let w = Wire::deserialize(d)?;
        let source = match (w.source, w.location) {
            (Some(s), _) => s,
            (None, Some(p)) => SourceKind::File(p),
            (None, None) => {
                return Err(serde::de::Error::custom(
                    "slot needs either `source` or `location`",
                ))
            }
        };
        Ok(Slot {
            source,
            name: w.name,
            start: w.start,
            end: w.end,
            length: w.length,
            rate: w.rate,
        })
    }
}

fn default_unset() -> f64 {
    -1.0
}
fn default_rate() -> f32 {
    1.0
}

pub const SLOTS_PER_BANK: usize = 10;
pub const MAX_BANKS: u8 = 26;

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
    pub last_error: Option<String>,

    // Phase 2 — conjur
    pub shader_banks: Vec<ShaderBank>,
    pub shader_bank_number: u8,
    pub shader_focus: u8,
    pub gles_profile: GlesProfile,
    /// Browser-selected shader name awaiting a slot mapping (set by SHADERS
    /// browser, consumed by Action::SelectShaderSlot when function_on is set).
    pub shader_pending_select: Option<String>,
    /// Currently triggered shader-bank slot (0..=9). None = bypass.
    pub shader_active_slot: Option<u8>,

    // Codec probe (Phase 2 sub-plan C)
    /// Codec-probe cache, populated by main.rs from the worker channel.
    pub probe_cache: ProbeCache,
    /// Sender to the probe worker. None outside main (e.g. in tests).
    pub probe_tx: Option<crossbeam_channel::Sender<ProbeRequest>>,

    // Detour (Phase 3)
    pub detour: DetourSettings,
    /// Display mode active before entering DetourScrub, restored on DetourExit.
    pub display_mode_before_detour: Option<DisplayMode>,
    /// Latest ring stats, updated each frame by main.rs.
    pub frames_stats_count: usize,
    pub frames_stats_capacity: usize,
    pub frames_stats_used_mb: u64,
    pub frames_stats_budget_mb: u64,
    pub frames_stats_fps: u32,

    // Phase 4b — captur (recording)
    pub active_recording: Option<crate::capture::recording::ActiveRecording>,
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
            last_error: None,
            shader_banks: vec![ShaderBank::empty()],
            shader_bank_number: 0,
            shader_focus: 0,
            gles_profile: GlesProfile::default_for_build(),
            shader_pending_select: None,
            shader_active_slot: None,
            probe_cache: ProbeCache::default(),
            probe_tx: None,
            detour: DetourSettings::default(),
            display_mode_before_detour: None,
            frames_stats_count: 0,
            frames_stats_capacity: 0,
            frames_stats_used_mb: 0,
            frames_stats_budget_mb: 0,
            frames_stats_fps: 30,
            active_recording: None,
        }
    }

    pub fn current_bank(&self) -> &Bank {
        &self.banks[self.bank_number as usize]
    }

    pub fn current_bank_mut(&mut self) -> &mut Bank {
        let n = self.bank_number as usize;
        &mut self.banks[n]
    }

    pub fn current_shader_bank(&self) -> &ShaderBank {
        &self.shader_banks[self.shader_bank_number as usize]
    }

    pub fn current_shader_bank_mut(&mut self) -> &mut ShaderBank {
        let n = self.shader_bank_number as usize;
        &mut self.shader_banks[n]
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
            source: SourceKind::File("/tmp/a.mp4".into()),
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
                source: SourceKind::File(format!("/tmp/{}.mp4", i).into()),
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

    #[test]
    fn shared_state_starts_with_no_error() {
        let s = SharedState::new();
        assert!(s.last_error.is_none());
    }

    #[test]
    fn shared_state_has_empty_shader_bank_and_default_profile() {
        let s = SharedState::new();
        assert_eq!(s.shader_banks.len(), 1);
        assert_eq!(s.shader_banks[0].slots.len(), 10);
        assert!(s.shader_banks[0].slots.iter().all(Option::is_none));
        assert_eq!(s.shader_bank_number, 0);
        assert_eq!(s.shader_focus, 0);
        assert_eq!(s.gles_profile, crate::render::shader_assembly::GlesProfile::default_for_build());
    }

    #[test]
    fn current_shader_bank_returns_active_bank() {
        let s = SharedState::new();
        assert_eq!(s.current_shader_bank().slots.len(), 10);
    }

    #[test]
    fn shared_state_starts_with_empty_probe_cache_and_no_tx() {
        let s = SharedState::new();
        assert!(s.probe_cache.is_empty());
        assert!(s.probe_tx.is_none());
    }

    #[test]
    fn shared_state_starts_with_default_detour_settings() {
        let s = SharedState::new();
        assert_eq!(s.detour.speed, 1.0);
        assert!(s.detour.forward);
        assert!(!s.detour.auto_play);
    }

    #[test]
    fn shared_state_starts_with_no_active_recording() {
        let s = SharedState::new();
        assert!(s.active_recording.is_none());
    }
}
