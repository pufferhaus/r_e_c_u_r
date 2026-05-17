//! Action → SharedState mutations. The single mutation point in the system.

use crate::action::{Action, SettingId};
use crate::state::{
    Bank, ControlMode, DisplayMode, LoadNext, LoopType, OnFinish, OnLoad, OnStart, PlayerMode,
    SharedState, Slot, SLOTS_PER_BANK,
};

/// Side-effects the mutator needs to push at the player rack. Real
/// implementation in `crate::video::rack`. Tests use a no-op or spy.
pub trait RackHandle {
    fn reload_all(&mut self);

    /// Trigger playback of the given slot. Caller resolves slot data from
    /// SharedState and passes explicit bank/slot indices so the rack can track
    /// the binding without holding its own stale bank snapshot. `bank_snapshot`
    /// is a clone of the active bank so the rack can pre-queue the successor
    /// slot without holding a reference into SharedState.
    fn trigger_slot_with(&mut self, bank: u8, slot_idx: u8, slot: Slot, bank_snapshot: Bank);

    /// Current player's playback position in seconds. `None` if nothing is loaded.
    fn current_position(&self) -> Option<f64>;

    /// Which (bank, slot_index) the current player is playing.
    /// Used by SetLoopIn/Out/ClearLoop to know which state slot to mutate.
    fn current_binding(&self) -> Option<(u8, u8)>;

    fn toggle_play_pause_now(&mut self);
    fn seek_relative_now(&mut self, seconds: f64);
    fn set_rate_now(&mut self, rate: f32);
    fn trigger_shader(&mut self, name: &str, params: [f32; 8]);
    fn clear_shader(&mut self);
    /// Push only the param values; no shader compile / select. Used by the
    /// PARAM screen for live-edit while a shader is already active.
    fn set_shader_params(&mut self, params: [f32; 8]);
    fn detour_scrub_by(&mut self, delta: i32);

    /// Phase 4b — recording. Attach a record bin to the player whose loaded
    /// slot's source is `Capture(d)` with `d.path == device_path`. The
    /// `file_path` is the absolute output path. Returns `Err` if no matching
    /// player is found or the bin attach fails.
    fn start_recording(
        &mut self,
        device_path: &str,
        file_path: &std::path::Path,
        target: crate::capture::recording::Target,
    ) -> crate::error::Result<()>;

    /// Phase 4b — signal stop on whichever player is recording. Sends EOS to
    /// the record bin and arranges for the finalized file path to be pushed
    /// onto `drain_finalized()` once the EOS round-trip completes.
    fn stop_recording(&mut self);

    /// Phase 4b — pop any newly-finalized file paths since the last call.
    /// Main loop drains these and calls `auto_import`.
    fn drain_finalized(&mut self) -> Vec<std::path::PathBuf>;
}

pub fn apply<R: RackHandle>(action: Action, state: &mut SharedState, rack: &mut R) {
    match action {
        Action::NavUp | Action::NavDown | Action::NavLeft | Action::NavRight | Action::Enter => {
            // Menu screens consume these directly; apply is a no-op here.
            // (Wired in Task 12 when ScreenStack lands.)
        }
        Action::Back => {
            // Pop screen / fallback handled by ScreenStack. No state mutation.
        }
        Action::Panic => {
            rack.reload_all();
            state.display_mode = DisplayMode::Sampler;
            state.control_mode = ControlMode::Default;
            state.function_on = false;
        }
        Action::EnterMode(m) => {
            state.display_mode = m;
        }
        Action::ToggleNowNext => {
            state.player_mode = match state.player_mode {
                PlayerMode::Now => PlayerMode::Next,
                PlayerMode::Next => PlayerMode::Now,
            };
        }
        Action::ToggleFunction => {
            state.function_on = !state.function_on;
        }
        Action::SelectSlot(n) => {
            // Gating: Fn → map highlighted browser row → slot.
            // Plain   → trigger slot n.
            // Mapping path is implemented when BrowserBody lands (Task 12);
            // for now plain trigger is enough for apply unit-tests.
            let n = n.min((SLOTS_PER_BANK - 1) as u8) as usize;
            if !state.function_on {
                let bank_idx = state.bank_number;
                if let Some(bank) = state.banks.get(bank_idx as usize).cloned() {
                    if let Some(slot) = bank.slots.get(n).cloned().flatten() {
                        rack.trigger_slot_with(bank_idx, n as u8, slot, bank);
                    }
                }
            }
            state.function_on = false;
        }
        Action::PrevBank => {
            if state.bank_number > 0 {
                state.bank_number -= 1;
            }
        }
        Action::NextBank => {
            use crate::state::Bank;
            if (state.bank_number as usize) + 1 >= crate::state::MAX_BANKS as usize {
                return; // already at last bank, no-op
            }
            if (state.bank_number as usize) + 1 >= state.banks.len() {
                state.banks.push(Bank::empty());
            }
            state.bank_number += 1;
        }
        Action::SetLoopIn => {
            if let (Some(pos), Some((b, s))) = (rack.current_position(), rack.current_binding()) {
                if let Some(Some(slot)) = state.banks.get_mut(b as usize)
                    .and_then(|bank| bank.slots.get_mut(s as usize))
                {
                    // Only accept if pos is before the current end (or end is unset).
                    if slot.end < 0.0 || pos < slot.end {
                        slot.start = pos;
                    }
                }
            }
        }
        Action::SetLoopOut => {
            if let (Some(pos), Some((b, s))) = (rack.current_position(), rack.current_binding()) {
                if let Some(Some(slot)) = state.banks.get_mut(b as usize)
                    .and_then(|bank| bank.slots.get_mut(s as usize))
                {
                    // Only accept if pos is after the current start (or start is unset).
                    if slot.start < 0.0 || pos > slot.start {
                        slot.end = pos;
                    }
                }
            }
        }
        Action::ClearLoop => {
            if let Some((b, s)) = rack.current_binding() {
                if let Some(slot) = state.banks.get_mut(b as usize)
                    .and_then(|bank| bank.slots.get_mut(s as usize))
                    .and_then(|opt| opt.as_mut())
                {
                    slot.start = -1.0;
                    slot.end = -1.0;
                }
            }
            rack.reload_all();
        }
        Action::TogglePlayPause => rack.toggle_play_pause_now(),
        Action::SeekRelative(s) => rack.seek_relative_now(s),
        Action::SetRate(r) => rack.set_rate_now(r),
        Action::Reload => rack.reload_all(),
        Action::CycleSetting(id) => cycle_setting(state, id),
        Action::TriggerShaderSlot(n) => {
            let n = (n as usize).min(crate::shader::SHADER_SLOTS_PER_BANK - 1);
            let slot = state.current_shader_bank().slots.get(n).cloned().flatten();
            match slot {
                Some(slot) => {
                    rack.trigger_shader(&slot.shader, slot.params);
                    state.shader_active_slot = Some(n as u8);
                }
                None => {
                    rack.clear_shader();
                    state.shader_active_slot = None;
                }
            }
            state.function_on = false;
        }
        Action::SelectShaderSlot(n) => {
            let n = (n as usize).min(crate::shader::SHADER_SLOTS_PER_BANK - 1);
            if state.function_on {
                if let Some(name) = state.shader_pending_select.take() {
                    let bank = state.current_shader_bank_mut();
                    bank.slots[n] = Some(crate::shader::ShaderSlot {
                        shader: name,
                        params: [0.0; 8],
                    });
                }
            }
            state.function_on = false;
        }
        Action::ShaderParamSelect(n) => {
            state.shader_focus = n.min(7);
        }
        Action::DetourEnter => {
            state.display_mode_before_detour = Some(state.display_mode);
            state.control_mode = ControlMode::DetourScrub;
            state.display_mode = DisplayMode::Frames;
        }
        Action::DetourExit => {
            state.control_mode = ControlMode::Default;
            if let Some(prev) = state.display_mode_before_detour.take() {
                state.display_mode = prev;
            }
        }
        Action::DetourScrubBy(delta) => {
            if state.control_mode == ControlMode::DetourScrub {
                rack.detour_scrub_by(delta);
            }
        }
        Action::DetourCycleSpeed => {
            if state.control_mode == ControlMode::DetourScrub {
                state.detour.cycle_speed();
            }
        }
        Action::DetourToggleDirection => {
            if state.control_mode == ControlMode::DetourScrub {
                state.detour.toggle_direction();
            }
        }
        Action::DetourTogglePlay => {
            if state.control_mode == ControlMode::DetourScrub {
                state.detour.toggle_play();
            }
        }
        Action::DetourSetStartMarker => {
            if state.control_mode == ControlMode::DetourScrub {
                state.detour.set_start_marker();
            }
        }
        Action::DetourSetEndMarker => {
            if state.control_mode == ControlMode::DetourScrub {
                state.detour.set_end_marker();
            }
        }
        Action::DetourClearMarkers => {
            if state.control_mode == ControlMode::DetourScrub {
                state.detour.clear_markers();
            }
        }
        Action::DetourCycleMix => {
            if state.control_mode == ControlMode::DetourScrub {
                state.detour.cycle_mix();
            }
        }
        Action::AddCaptureSlot => {
            let devs = crate::capture::enumerate_capture_devices();
            if let Some(d) = devs.into_iter().next() {
                if let Some(idx) = state.current_bank().first_empty() {
                    let label = d.label.clone();
                    state.current_bank_mut().slots[idx] = Some(crate::state::Slot {
                        source: crate::state::SourceKind::Capture(d),
                        name: label,
                        start: -1.0,
                        end: -1.0,
                        length: 0.0,
                        rate: 1.0,
                    });
                }
            } else {
                state.last_error = Some("no capture devices found".to_string());
            }
        }
        Action::RecordToggle => {
            use crate::capture::recording::{
                ActiveRecording, RecState, Target,
                check_disk_space, generate_recording_path,
            };
            use std::time::Instant;

            // Determine recording directory.
            let dir = state
                .paths_to_browser
                .first()
                .cloned()
                .unwrap_or_else(|| std::env::temp_dir().join("recur-recordings"))
                .join("recordings");

            // Branch on current recording state.
            match state.active_recording.as_ref().map(|r| r.state) {
                Some(RecState::Finalizing) => {
                    state.last_error = Some("still saving previous recording".to_string());
                }
                Some(RecState::Recording) => {
                    rack.stop_recording();
                    if let Some(rec) = state.active_recording.as_mut() {
                        rec.state = RecState::Finalizing;
                    }
                }
                None => {
                    // Discover the active capture device by checking the rack's
                    // current binding against the bank's slot table.
                    let device_path: Option<String> = rack.current_binding().and_then(|(b, s)| {
                        state.banks
                            .get(b as usize)?
                            .slots
                            .get(s as usize)?
                            .as_ref()
                            .and_then(|slot| match &slot.source {
                                crate::state::SourceKind::Capture(d) => Some(d.path.clone()),
                                _ => None,
                            })
                    });
                    let Some(device_path) = device_path else {
                        state.last_error = Some("no active capture source".to_string());
                        return;
                    };

                    // Disk-space gate.
                    if !check_disk_space(&dir, 10) {
                        state.last_error = Some("insufficient space on disk".to_string());
                        return;
                    }
                    if let Err(e) = std::fs::create_dir_all(&dir) {
                        state.last_error = Some(format!("recording: {e}"));
                        return;
                    }

                    // Filename.
                    let date = today_yyyymmdd();
                    let file_path = generate_recording_path(&dir, &date);
                    let target = Target::current();
                    match rack.start_recording(&device_path, &file_path, target) {
                        Ok(()) => {
                            let now = Instant::now();
                            state.active_recording = Some(ActiveRecording {
                                device_path,
                                file_path,
                                started_at: now,
                                state: RecState::Recording,
                                last_disk_check: now,
                            });
                        }
                        Err(e) => {
                            state.last_error = Some(format!("recording: {e}"));
                        }
                    }
                }
            }
        }
        Action::ShaderParamAdjust(delta) => {
            if state.control_mode != ControlMode::ShaderParam {
                return;
            }
            let Some(active) = state.shader_active_slot else { return; };
            let bank_idx = state.shader_bank_number as usize;
            let focus = state.shader_focus as usize;
            let mut updated_params: Option<[f32; 8]> = None;
            if let Some(Some(slot)) = state
                .shader_banks
                .get_mut(bank_idx)
                .and_then(|b| b.slots.get_mut(active as usize))
            {
                // Step = 1% of [-1.0, 1.0] range. Starter shaders all expect
                // params in that range.
                let step = 0.02_f32 * delta as f32;
                let v = (slot.params[focus] + step).clamp(-1.0, 1.0);
                slot.params[focus] = v;
                updated_params = Some(slot.params);
            }
            if let Some(p) = updated_params {
                rack.set_shader_params(p);
            }
        }
    }
}

fn cycle_setting(state: &mut SharedState, id: SettingId) {
    let s = &mut state.sampler;
    match id {
        SettingId::LoopType => {
            s.loop_type = match s.loop_type {
                LoopType::Sequential => LoopType::Parallel,
                LoopType::Parallel => LoopType::Sequential,
            }
        }
        SettingId::OnFinish => {
            s.on_finish = match s.on_finish {
                OnFinish::Switch => OnFinish::Repeat,
                OnFinish::Repeat => OnFinish::Switch,
            }
        }
        SettingId::OnStart => {
            s.on_start = match s.on_start {
                OnStart::Play => OnStart::Show,
                OnStart::Show => OnStart::PlayShow,
                OnStart::PlayShow => OnStart::Play,
            }
        }
        SettingId::OnLoad => {
            s.on_load = match s.on_load {
                OnLoad::Show => OnLoad::Hide,
                OnLoad::Hide => OnLoad::Show,
            }
        }
        SettingId::LoadNext => {
            s.load_next = match s.load_next {
                LoadNext::Auto => LoadNext::Manual,
                LoadNext::Manual => LoadNext::Auto,
            }
        }
        SettingId::RandStartMode => s.rand_start_mode = !s.rand_start_mode,
        SettingId::FixedLengthMode => s.fixed_length_mode = !s.fixed_length_mode,
        SettingId::FixedLengthMultiply => {
            // cycle through 0.5x, 1x, 2x, 4x
            s.fixed_length_multiply = match s.fixed_length_multiply {
                m if (m - 0.5).abs() < 0.01 => 1.0,
                m if (m - 1.0).abs() < 0.01 => 2.0,
                m if (m - 2.0).abs() < 0.01 => 4.0,
                _ => 0.5,
            };
        }
        SettingId::ResetPlayers => s.reset_players = !s.reset_players,
    }
}

/// Phase 4b — wire a finalized recording file into the first empty slot of
/// the current bank. Called by the main loop on each finalize event drained
/// from the rack. If no empty slot is available, leaves the file on disk and
/// sets `last_error`. Caller is responsible for clearing `active_recording`
/// before/after invoking this.
pub fn auto_import_recording(state: &mut SharedState, file_path: std::path::PathBuf) {
    let Some(idx) = state.current_bank().first_empty() else {
        let base = file_path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "?".into());
        state.last_error = Some(format!("recording saved: {base} (no empty slot)"));
        return;
    };
    let name = file_path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "recording.mp4".into());
    state.current_bank_mut().slots[idx] = Some(crate::state::Slot {
        source: crate::state::SourceKind::File(file_path),
        name,
        start: -1.0,
        end: -1.0,
        length: 0.0,
        rate: 1.0,
    });
}

/// Pure Howard-Hinnant civil-date conversion: Unix-epoch days → "YYYY-MM-DD".
fn days_to_yyyymmdd(days: u64) -> String {
    let z = days as i64 + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{:04}-{:02}-{:02}", y, m, d)
}

fn today_yyyymmdd() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    days_to_yyyymmdd(secs / 86400)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{Slot, SourceKind};

    #[derive(Default, Debug)]
    struct SpyRack {
        reload_count: u32,
        /// (bank, slot_idx, slot_name) for each trigger_slot_with call.
        trigger_calls: Vec<(u8, u8, String)>,
        toggle_pause: u32,
        /// Reported by current_position().
        position: Option<f64>,
        /// Reported by current_binding().
        binding: Option<(u8, u8)>,
        shader_triggers: Vec<String>,
        shader_cleared: u32,
        shader_param_pushes: Vec<[f32; 8]>,
        detour_scrubs: Vec<i32>,
        record_starts: u32,
        record_stops: u32,
        finalized: Vec<std::path::PathBuf>,
    }

    impl RackHandle for SpyRack {
        fn reload_all(&mut self) {
            self.reload_count += 1;
        }
        fn trigger_slot_with(&mut self, bank: u8, slot_idx: u8, slot: Slot, _bank_snapshot: crate::state::Bank) {
            self.trigger_calls.push((bank, slot_idx, slot.name.clone()));
            self.binding = Some((bank, slot_idx));
        }
        fn current_position(&self) -> Option<f64> {
            self.position
        }
        fn current_binding(&self) -> Option<(u8, u8)> {
            self.binding
        }
        fn toggle_play_pause_now(&mut self) {
            self.toggle_pause += 1;
        }
        fn seek_relative_now(&mut self, _: f64) {}
        fn set_rate_now(&mut self, _: f32) {}
        fn trigger_shader(&mut self, name: &str, _params: [f32; 8]) {
            self.shader_triggers.push(name.to_string());
        }
        fn clear_shader(&mut self) {
            self.shader_cleared += 1;
        }
        fn set_shader_params(&mut self, params: [f32; 8]) {
            self.shader_param_pushes.push(params);
        }
        fn detour_scrub_by(&mut self, delta: i32) {
            self.detour_scrubs.push(delta);
        }
        fn start_recording(
            &mut self,
            _device_path: &str,
            _file_path: &std::path::Path,
            _target: crate::capture::recording::Target,
        ) -> crate::error::Result<()> {
            self.record_starts += 1;
            Ok(())
        }
        fn stop_recording(&mut self) {
            self.record_stops += 1;
        }
        fn drain_finalized(&mut self) -> Vec<std::path::PathBuf> {
            std::mem::take(&mut self.finalized)
        }
    }

    use crate::shader::ShaderSlot;

    #[test]
    fn select_shader_slot_with_function_off_triggers_pulse() {
        let mut s = SharedState::new();
        s.current_shader_bank_mut().slots[2] = Some(ShaderSlot {
            shader: "color_shift".into(),
            params: [0.0; 8],
        });
        let mut r = SpyRack::default();
        apply(Action::TriggerShaderSlot(2), &mut s, &mut r);
        assert_eq!(r.shader_triggers, vec!["color_shift".to_string()]);
    }

    #[test]
    fn trigger_shader_slot_empty_clears_active() {
        let mut s = SharedState::new();
        let mut r = SpyRack::default();
        apply(Action::TriggerShaderSlot(5), &mut s, &mut r);
        assert_eq!(r.shader_cleared, 1);
    }

    #[test]
    fn select_shader_slot_with_function_on_maps_into_slot() {
        let mut s = SharedState::new();
        s.function_on = true;
        s.shader_focus = 4;
        s.shader_pending_select = Some("pixelate".to_string());
        let mut r = SpyRack::default();
        apply(Action::SelectShaderSlot(3), &mut s, &mut r);
        let slot = s.current_shader_bank().slots[3].as_ref().unwrap();
        assert_eq!(slot.shader, "pixelate");
        assert!(!s.function_on);
    }

    #[test]
    fn shader_param_adjust_clamped_by_meta() {
        let mut s = SharedState::new();
        s.control_mode = ControlMode::ShaderParam;
        s.shader_focus = 0;
        s.current_shader_bank_mut().slots[0] = Some(ShaderSlot {
            shader: "color_shift".into(),
            params: [0.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        });
        s.shader_active_slot = Some(0);
        let mut r = SpyRack::default();
        apply(Action::ShaderParamAdjust(1), &mut s, &mut r);
        let slot = s.current_shader_bank().slots[0].as_ref().unwrap();
        assert!(slot.params[0] > 0.5);
    }

    fn make_slot(name: &str) -> Slot {
        Slot {
            source: SourceKind::File(format!("/tmp/{}.mp4", name).into()),
            name: name.into(),
            start: -1.0,
            end: -1.0,
            length: 10.0,
            rate: 1.0,
        }
    }

    #[test]
    fn panic_resets_state_and_rack() {
        let mut s = SharedState::new();
        s.display_mode = DisplayMode::Browser;
        s.function_on = true;
        let mut r = SpyRack::default();
        apply(Action::Panic, &mut s, &mut r);
        assert_eq!(s.display_mode, DisplayMode::Sampler);
        assert!(!s.function_on);
        assert_eq!(r.reload_count, 1);
    }

    #[test]
    fn select_slot_triggers_when_function_off() {
        let mut s = SharedState::new();
        s.banks[0].slots[3] = Some(make_slot("clip3"));
        let mut r = SpyRack::default();
        apply(Action::SelectSlot(3), &mut s, &mut r);
        assert_eq!(r.trigger_calls, vec![(0, 3, "clip3".to_string())]);
    }

    #[test]
    fn select_slot_does_not_trigger_when_function_on() {
        let mut s = SharedState::new();
        s.function_on = true;
        let mut r = SpyRack::default();
        apply(Action::SelectSlot(3), &mut s, &mut r);
        assert!(r.trigger_calls.is_empty());
        assert!(!s.function_on, "function clears after slot key");
    }

    #[test]
    fn select_slot_is_noop_when_slot_empty() {
        // slot 5 is None — no trigger call expected.
        let mut s = SharedState::new();
        let mut r = SpyRack::default();
        apply(Action::SelectSlot(5), &mut s, &mut r);
        assert!(r.trigger_calls.is_empty());
    }

    #[test]
    fn next_bank_grows_banks_vec() {
        let mut s = SharedState::new();
        let mut r = SpyRack::default();
        apply(Action::NextBank, &mut s, &mut r);
        assert_eq!(s.bank_number, 1);
        assert_eq!(s.banks.len(), 2);
    }

    #[test]
    fn next_bank_caps_at_max() {
        let mut s = SharedState::new();
        let mut r = SpyRack::default();
        for _ in 0..50 {
            apply(Action::NextBank, &mut s, &mut r);
        }
        assert_eq!(s.bank_number as usize, crate::state::MAX_BANKS as usize - 1);
        assert!(s.banks.len() <= crate::state::MAX_BANKS as usize);
    }

    #[test]
    fn prev_bank_clamps_at_zero() {
        let mut s = SharedState::new();
        let mut r = SpyRack::default();
        apply(Action::PrevBank, &mut s, &mut r);
        assert_eq!(s.bank_number, 0);
    }

    #[test]
    fn toggle_now_next_round_trips() {
        let mut s = SharedState::new();
        let mut r = SpyRack::default();
        apply(Action::ToggleNowNext, &mut s, &mut r);
        assert_eq!(s.player_mode, PlayerMode::Next);
        apply(Action::ToggleNowNext, &mut s, &mut r);
        assert_eq!(s.player_mode, PlayerMode::Now);
    }

    #[test]
    fn cycle_loop_type_alternates() {
        let mut s = SharedState::new();
        let mut r = SpyRack::default();
        apply(Action::CycleSetting(SettingId::LoopType), &mut s, &mut r);
        assert_eq!(s.sampler.loop_type, LoopType::Parallel);
        apply(Action::CycleSetting(SettingId::LoopType), &mut s, &mut r);
        assert_eq!(s.sampler.loop_type, LoopType::Sequential);
    }

    #[test]
    fn fixed_length_multiply_cycle_loops() {
        let mut s = SharedState::new();
        let mut r = SpyRack::default();
        let m = SettingId::FixedLengthMultiply;
        apply(Action::CycleSetting(m), &mut s, &mut r);
        assert_eq!(s.sampler.fixed_length_multiply, 2.0);
        apply(Action::CycleSetting(m), &mut s, &mut r);
        assert_eq!(s.sampler.fixed_length_multiply, 4.0);
        apply(Action::CycleSetting(m), &mut s, &mut r);
        assert_eq!(s.sampler.fixed_length_multiply, 0.5);
    }

    #[test]
    fn set_loop_in_writes_to_state_slot() {
        let mut s = SharedState::new();
        s.banks[0].slots[2] = Some(Slot {
            source: SourceKind::File("/tmp/x.mp4".into()),
            name: "x".into(),
            start: -1.0,
            end: 5.0,
            length: 10.0,
            rate: 1.0,
        });
        let mut r = SpyRack::default();
        r.position = Some(1.5);
        r.binding = Some((0, 2));
        apply(Action::SetLoopIn, &mut s, &mut r);
        assert_eq!(s.banks[0].slots[2].as_ref().unwrap().start, 1.5);
    }

    #[test]
    fn set_loop_in_rejects_when_pos_past_end() {
        let mut s = SharedState::new();
        s.banks[0].slots[2] = Some(Slot {
            source: SourceKind::File("/tmp/x.mp4".into()),
            name: "x".into(),
            start: 0.0,
            end: 3.0,
            length: 10.0,
            rate: 1.0,
        });
        let mut r = SpyRack::default();
        r.position = Some(5.0);
        r.binding = Some((0, 2));
        apply(Action::SetLoopIn, &mut s, &mut r);
        // pos 5.0 > end 3.0 — rejected; start stays at 0.0.
        assert_eq!(s.banks[0].slots[2].as_ref().unwrap().start, 0.0);
    }

    #[test]
    fn clear_loop_resets_both_endpoints() {
        let mut s = SharedState::new();
        s.banks[0].slots[2] = Some(Slot {
            source: SourceKind::File("/tmp/x.mp4".into()),
            name: "x".into(),
            start: 1.0,
            end: 4.0,
            length: 10.0,
            rate: 1.0,
        });
        let mut r = SpyRack::default();
        r.binding = Some((0, 2));
        apply(Action::ClearLoop, &mut s, &mut r);
        let slot = s.banks[0].slots[2].as_ref().unwrap();
        assert_eq!(slot.start, -1.0);
        assert_eq!(slot.end, -1.0);
        assert_eq!(r.reload_count, 1);
    }

    #[test]
    fn trigger_shader_slot_clears_function_latch() {
        let mut s = SharedState::new();
        s.function_on = true;
        let mut r = SpyRack::default();
        apply(Action::TriggerShaderSlot(0), &mut s, &mut r);
        assert!(!s.function_on, "TriggerShaderSlot must clear function_on like SelectSlot does");
    }

    #[test]
    fn shader_param_adjust_pushes_params_to_rack() {
        let mut s = SharedState::new();
        s.control_mode = ControlMode::ShaderParam;
        s.shader_focus = 1;
        s.shader_active_slot = Some(0);
        s.current_shader_bank_mut().slots[0] = Some(ShaderSlot {
            shader: "color_shift".into(),
            params: [0.0, 0.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        });
        let mut r = SpyRack::default();
        apply(Action::ShaderParamAdjust(1), &mut s, &mut r);
        assert_eq!(r.shader_param_pushes.len(), 1);
        let pushed = r.shader_param_pushes[0];
        assert!((pushed[1] - 0.52).abs() < 1e-5, "got {pushed:?}");
    }

    #[test]
    fn detour_enter_sets_control_mode_and_display_mode() {
        let mut s = SharedState::new();
        let mut r = SpyRack::default();
        apply(Action::DetourEnter, &mut s, &mut r);
        assert_eq!(s.control_mode, ControlMode::DetourScrub);
        assert_eq!(s.display_mode, DisplayMode::Frames);
    }

    #[test]
    fn detour_exit_resets_control_mode_to_default() {
        let mut s = SharedState::new();
        s.control_mode = ControlMode::DetourScrub;
        let mut r = SpyRack::default();
        apply(Action::DetourExit, &mut s, &mut r);
        assert_eq!(s.control_mode, ControlMode::Default);
    }

    #[test]
    fn detour_exit_restores_prior_display_mode() {
        let mut s = SharedState::new();
        s.display_mode = DisplayMode::Sampler;
        let mut r = SpyRack::default();
        apply(Action::DetourEnter, &mut s, &mut r);
        assert_eq!(s.display_mode, DisplayMode::Frames);
        apply(Action::DetourExit, &mut s, &mut r);
        assert_eq!(s.display_mode, DisplayMode::Sampler);
        assert!(s.display_mode_before_detour.is_none());
    }

    #[test]
    fn detour_scrub_by_outside_scrub_mode_is_noop() {
        let mut s = SharedState::new();
        let mut r = SpyRack::default();
        apply(Action::DetourScrubBy(2), &mut s, &mut r);
        assert!(r.detour_scrubs.is_empty());
    }

    #[test]
    fn detour_scrub_by_in_scrub_mode_pushes_delta_to_rack() {
        let mut s = SharedState::new();
        s.control_mode = ControlMode::DetourScrub;
        let mut r = SpyRack::default();
        apply(Action::DetourScrubBy(-3), &mut s, &mut r);
        assert_eq!(r.detour_scrubs, vec![-3]);
    }

    #[test]
    fn detour_cycle_speed_advances_through_cycle() {
        let mut s = SharedState::new();
        s.control_mode = ControlMode::DetourScrub;
        let mut r = SpyRack::default();
        s.detour.speed = 1.0;
        apply(Action::DetourCycleSpeed, &mut s, &mut r);
        assert!((s.detour.speed - 2.0).abs() < 1e-6);
    }

    #[test]
    fn detour_cycle_mix_advances() {
        let mut s = SharedState::new();
        s.control_mode = ControlMode::DetourScrub;
        let mut r = SpyRack::default();
        s.detour.mix = 0.0;
        apply(Action::DetourCycleMix, &mut s, &mut r);
        assert!((s.detour.mix - 0.25).abs() < 1e-6);
    }

    #[test]
    fn detour_toggle_play_flips_auto_play() {
        let mut s = SharedState::new();
        s.control_mode = ControlMode::DetourScrub;
        let mut r = SpyRack::default();
        assert!(!s.detour.auto_play);
        apply(Action::DetourTogglePlay, &mut s, &mut r);
        assert!(s.detour.auto_play);
    }

    #[test]
    fn detour_markers_set_and_clear() {
        let mut s = SharedState::new();
        s.control_mode = ControlMode::DetourScrub;
        s.detour.read_position = 7;
        let mut r = SpyRack::default();
        apply(Action::DetourSetStartMarker, &mut s, &mut r);
        assert_eq!(s.detour.start_marker, Some(7));
        s.detour.read_position = 42;
        apply(Action::DetourSetEndMarker, &mut s, &mut r);
        assert_eq!(s.detour.end_marker, Some(42));
        apply(Action::DetourClearMarkers, &mut s, &mut r);
        assert!(s.detour.start_marker.is_none());
        assert!(s.detour.end_marker.is_none());
    }

    fn capture_slot(path: &str) -> Slot {
        Slot {
            source: crate::state::SourceKind::Capture(crate::capture::CaptureDevice {
                path: path.into(),
                label: format!("test:{path}"),
            }),
            name: format!("test:{path}"),
            start: -1.0, end: -1.0, length: 0.0, rate: 1.0,
        }
    }

    #[test]
    fn record_toggle_with_no_active_capture_sets_last_error() {
        let mut s = SharedState::new();
        s.paths_to_browser = vec![std::env::temp_dir()];
        let mut r = SpyRack::default();
        apply(Action::RecordToggle, &mut s, &mut r);
        let err = s.last_error.as_deref().unwrap_or("");
        assert!(err.contains("no active capture source"), "got: {err:?}");
        assert!(s.active_recording.is_none());
        assert_eq!(r.record_starts, 0);
    }

    #[test]
    fn record_toggle_with_active_capture_starts_recording() {
        let mut s = SharedState::new();
        let tmp = tempfile::TempDir::new().unwrap();
        s.paths_to_browser = vec![tmp.path().to_path_buf()];
        s.banks[0].slots[0] = Some(capture_slot("/dev/video0"));
        let mut r = SpyRack::default();
        // Trigger slot 0 first so the rack knows it's the active source.
        apply(Action::SelectSlot(0), &mut s, &mut r);
        apply(Action::RecordToggle, &mut s, &mut r);
        assert!(s.active_recording.is_some(), "last_error: {:?}", s.last_error);
        let rec = s.active_recording.as_ref().unwrap();
        assert_eq!(rec.device_path, "/dev/video0");
        assert!(rec.file_path.starts_with(tmp.path().join("recordings")),
            "file_path: {:?}", rec.file_path);
        assert_eq!(r.record_starts, 1);
    }

    #[test]
    fn second_record_toggle_while_recording_stops_it() {
        let mut s = SharedState::new();
        let tmp = tempfile::TempDir::new().unwrap();
        s.paths_to_browser = vec![tmp.path().to_path_buf()];
        s.banks[0].slots[0] = Some(capture_slot("/dev/video0"));
        let mut r = SpyRack::default();
        apply(Action::SelectSlot(0), &mut s, &mut r);
        apply(Action::RecordToggle, &mut s, &mut r);
        apply(Action::RecordToggle, &mut s, &mut r);
        let rec = s.active_recording.as_ref().expect("still tracking until finalize");
        assert_eq!(rec.state, crate::capture::recording::RecState::Finalizing);
        assert_eq!(r.record_stops, 1);
    }

    #[test]
    fn record_toggle_during_finalize_sets_still_saving_error() {
        let mut s = SharedState::new();
        let tmp = tempfile::TempDir::new().unwrap();
        s.paths_to_browser = vec![tmp.path().to_path_buf()];
        s.banks[0].slots[0] = Some(capture_slot("/dev/video0"));
        let mut r = SpyRack::default();
        apply(Action::SelectSlot(0), &mut s, &mut r);
        apply(Action::RecordToggle, &mut s, &mut r);
        apply(Action::RecordToggle, &mut s, &mut r); // -> Finalizing
        apply(Action::RecordToggle, &mut s, &mut r); // -> refused
        let err = s.last_error.as_deref().unwrap_or("");
        assert!(err.contains("still saving"), "got: {err:?}");
        assert_eq!(r.record_stops, 1, "should not call stop twice");
    }

    #[test]
    fn add_capture_slot_populates_first_empty_when_devices_present() {
        let mut s = SharedState::new();
        let mut r = SpyRack::default();
        apply(Action::AddCaptureSlot, &mut s, &mut r);

        let devs = crate::capture::enumerate_capture_devices();
        if devs.is_empty() {
            // Linux CI with no /dev/video*: expect last_error set; no slot populated.
            assert!(s.banks[0].slots[0].is_none());
            assert!(s.last_error.as_deref().unwrap_or("").contains("no capture"));
        } else {
            // macOS / Linux with cameras: slot 0 populated with a Capture kind.
            let slot = s.banks[0].slots[0].as_ref().expect("slot 0 should populate");
            match &slot.source {
                crate::state::SourceKind::Capture(d) => {
                    assert_eq!(d.path, devs[0].path);
                }
                _ => panic!("expected Capture source"),
            }
        }
    }

    #[test]
    fn rack_handle_has_record_methods() {
        // Trait compiles with the new methods. Real test of the wiring is in Task 9.
        let mut r = SpyRack::default();
        let target = crate::capture::recording::Target::current();
        let ok = r.start_recording("/dev/video0", std::path::Path::new("/tmp/r.mp4"), target);
        assert!(ok.is_ok());
        assert_eq!(r.record_starts, 1);
        r.stop_recording();
        assert_eq!(r.record_stops, 1);
        let finalized: Vec<std::path::PathBuf> = r.drain_finalized();
        assert!(finalized.is_empty());
    }

    #[test]
    fn auto_import_recording_populates_first_empty_slot() {
        let mut s = SharedState::new();
        // Bank 0 slot 0 filled with a capture slot to ensure we skip past it.
        s.banks[0].slots[0] = Some(capture_slot("/dev/video0"));
        let path = std::path::PathBuf::from("/tmp/rec-2026-05-17-3.mp4");
        crate::apply::auto_import_recording(&mut s, path.clone());
        let slot1 = s.banks[0].slots[1].as_ref().expect("slot 1 populated");
        match &slot1.source {
            crate::state::SourceKind::File(p) => assert_eq!(p, &path),
            _ => panic!("expected File source"),
        }
        assert!(slot1.name.contains("rec-2026-05-17-3"));
    }

    #[test]
    fn auto_import_recording_when_bank_full_sets_last_error() {
        let mut s = SharedState::new();
        for i in 0..10 {
            s.banks[0].slots[i] = Some(capture_slot(&format!("/dev/video{i}")));
        }
        crate::apply::auto_import_recording(&mut s, "/tmp/rec-2026-05-17-7.mp4".into());
        let err = s.last_error.as_deref().unwrap_or("");
        assert!(err.contains("recording saved"), "got: {err:?}");
        assert!(err.contains("no empty slot"), "got: {err:?}");
    }

    #[test]
    fn days_to_yyyymmdd_epoch_is_1970_01_01() {
        assert_eq!(super::days_to_yyyymmdd(0), "1970-01-01");
    }

    #[test]
    fn days_to_yyyymmdd_today_2026_05_17() {
        // 2026-05-17 = day 20590 since Unix epoch.
        assert_eq!(super::days_to_yyyymmdd(20590), "2026-05-17");
    }

    #[test]
    fn days_to_yyyymmdd_year_rollover_2025_12_31() {
        // 2025-12-31 = day 20453.
        assert_eq!(super::days_to_yyyymmdd(20453), "2025-12-31");
    }

    #[test]
    fn days_to_yyyymmdd_leap_day_2024_02_29() {
        // 2024-02-29 = day 19782.
        assert_eq!(super::days_to_yyyymmdd(19782), "2024-02-29");
    }

    #[test]
    fn today_yyyymmdd_has_correct_shape() {
        let s = super::today_yyyymmdd();
        assert_eq!(s.len(), 10, "got: {s}");
        assert_eq!(&s[4..5], "-");
        assert_eq!(&s[7..8], "-");
    }
}
