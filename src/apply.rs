//! Action → SharedState mutations. The single mutation point in the system.

use crate::action::{Action, SettingId};
use crate::state::{
    Bank, ControlMode, DisplayMode, LoadNext, LoopType, OnFinish, OnLoad, OnStart, PlayerMode,
    SharedState, SLOTS_PER_BANK,
};

/// Side-effects the mutator needs to push at the player rack. Real
/// implementation in `crate::video::rack`. Tests use a no-op or spy.
pub trait RackHandle {
    fn reload_all(&mut self);
    fn trigger_slot(&mut self, bank: u8, slot: u8);
    fn set_loop_in_now(&mut self);
    fn set_loop_out_now(&mut self);
    fn toggle_play_pause_now(&mut self);
    fn seek_relative_now(&mut self, seconds: f64);
    fn set_rate_now(&mut self, rate: f32);
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
            let n = n.min((SLOTS_PER_BANK - 1) as u8);
            if !state.function_on {
                rack.trigger_slot(state.bank_number, n);
            }
            state.function_on = false;
        }
        Action::PrevBank => {
            if state.bank_number > 0 {
                state.bank_number -= 1;
            }
        }
        Action::NextBank => {
            if (state.bank_number as usize) + 1 >= state.banks.len() {
                state.banks.push(Bank::empty());
            }
            state.bank_number += 1;
        }
        Action::SetLoopIn => rack.set_loop_in_now(),
        Action::SetLoopOut => rack.set_loop_out_now(),
        Action::ClearLoop => {
            // ClearLoop applies to the slot the "now" player is bound to.
            // The slot location is owned by SharedState; rack drives playback.
            // We need both to be in sync: clear on state, then trigger reload.
            // For Phase 1 the rack reloads its current slot on next tick.
            rack.reload_all();
        }
        Action::TogglePlayPause => rack.toggle_play_pause_now(),
        Action::SeekRelative(s) => rack.seek_relative_now(s),
        Action::SetRate(r) => rack.set_rate_now(r),
        Action::Reload => rack.reload_all(),
        Action::CycleSetting(id) => cycle_setting(state, id),
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

    #[derive(Default, Debug)]
    struct SpyRack {
        reload_count: u32,
        trigger: Vec<(u8, u8)>,
        loop_in: u32,
        loop_out: u32,
        toggle_pause: u32,
    }

    impl RackHandle for SpyRack {
        fn reload_all(&mut self) {
            self.reload_count += 1;
        }
        fn trigger_slot(&mut self, b: u8, s: u8) {
            self.trigger.push((b, s));
        }
        fn set_loop_in_now(&mut self) {
            self.loop_in += 1;
        }
        fn set_loop_out_now(&mut self) {
            self.loop_out += 1;
        }
        fn toggle_play_pause_now(&mut self) {
            self.toggle_pause += 1;
        }
        fn seek_relative_now(&mut self, _: f64) {}
        fn set_rate_now(&mut self, _: f32) {}
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
        let mut r = SpyRack::default();
        apply(Action::SelectSlot(3), &mut s, &mut r);
        assert_eq!(r.trigger, vec![(0, 3)]);
    }

    #[test]
    fn select_slot_does_not_trigger_when_function_on() {
        let mut s = SharedState::new();
        s.function_on = true;
        let mut r = SpyRack::default();
        apply(Action::SelectSlot(3), &mut s, &mut r);
        assert!(r.trigger.is_empty());
        assert!(!s.function_on, "function clears after slot key");
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
}
