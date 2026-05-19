//! ParamBody — overlays in `ControlMode::ShaderParam`. Shows the 8 params of
//! the active shader slot and lets NavLeft/NavRight nudge values via
//! `Action::ShaderParamAdjust(±1)`. NavUp/Down move the focus.

use crate::action::Action;
use crate::state::{ControlMode, SharedState};
use crate::status::grid::TextGrid;
use crate::ui::{Screen, ScreenResult};

pub struct ParamBody;

impl ParamBody {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ParamBody {
    fn default() -> Self {
        Self::new()
    }
}

impl Screen for ParamBody {
    fn render(&self, state: &SharedState, grid: &mut TextGrid) {
        grid.write_row(4, "param  name              value");
        let Some(active) = state.shader_active_slot else {
            grid.write_row(6, "  (no shader slot active)");
            return;
        };
        let bank = state.current_shader_bank();
        let Some(Some(slot)) = bank.slots.get(active as usize) else {
            grid.write_row(6, "  (active slot empty)");
            return;
        };
        for i in 0..8 {
            let row = 5 + i;
            let line = format!(
                "{:^5}  {:<16}  {:>+.3}",
                i,
                format!("u_param{i}"),
                slot.params[i]
            );
            grid.write_row(row, &line);
            if i as u8 == state.shader_focus {
                grid.invert_row(row);
            }
        }
    }

    fn handle(&mut self, action: Action, state: &mut SharedState) -> ScreenResult {
        match action {
            Action::NavUp => {
                state.shader_focus = state.shader_focus.saturating_sub(1);
                ScreenResult::Continue
            }
            Action::NavDown => {
                state.shader_focus = (state.shader_focus + 1).min(7);
                ScreenResult::Continue
            }
            Action::NavLeft => ScreenResult::Action(Action::ShaderParamAdjust(-1)),
            Action::NavRight => ScreenResult::Action(Action::ShaderParamAdjust(1)),
            Action::Back => {
                state.control_mode = ControlMode::Default;
                ScreenResult::Continue
            }
            _ => ScreenResult::Continue,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn navleft_synthesises_param_adjust_minus() {
        let mut b = ParamBody::new();
        let mut s = SharedState::new();
        match b.handle(Action::NavLeft, &mut s) {
            ScreenResult::Action(Action::ShaderParamAdjust(-1)) => (),
            other => panic!("expected ShaderParamAdjust(-1), got something else (got Action variant via Action(...) wrapper or wrong variant)"),
        }
    }

    #[test]
    fn nav_down_moves_focus_clamped() {
        let mut b = ParamBody::new();
        let mut s = SharedState::new();
        for _ in 0..20 {
            b.handle(Action::NavDown, &mut s);
        }
        assert_eq!(s.shader_focus, 7);
    }

    #[test]
    fn back_exits_param_mode() {
        let mut b = ParamBody::new();
        let mut s = SharedState::new();
        s.control_mode = ControlMode::ShaderParam;
        b.handle(Action::Back, &mut s);
        assert_eq!(s.control_mode, ControlMode::Default);
    }
}
