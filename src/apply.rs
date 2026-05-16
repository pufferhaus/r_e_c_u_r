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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::Slot;

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
            location: format!("/tmp/{}.mp4", name).into(),
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
            location: "/tmp/x.mp4".into(),
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
            location: "/tmp/x.mp4".into(),
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
            location: "/tmp/x.mp4".into(),
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
}
