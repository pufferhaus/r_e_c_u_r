//! ShdrBnkBody — 10-slot grid for shader-slot assignments. Mirrors SamplerBody.

use crate::action::Action;
use crate::state::SharedState;
use crate::status::grid::TextGrid;
use crate::ui::{Screen, ScreenResult};

pub struct ShdrBnkBody {
    selected: u8,
}

impl ShdrBnkBody {
    pub fn new() -> Self {
        Self { selected: 0 }
    }
}

impl Screen for ShdrBnkBody {
    fn render(&self, state: &SharedState, grid: &mut TextGrid) {
        let bank = state.current_shader_bank();
        grid.write_row(4, &format!("{:>6} {:<28} {:<5}", format!("{}-slot", state.shader_bank_number), "shader", "act"));
        for (i, opt) in bank.slots.iter().enumerate() {
            let row_idx = 5 + i;
            let line = match opt {
                None => format!("{:^6} {:<28} {:<5}", i, "", ""),
                Some(s) => {
                    let active_marker = if state.shader_active_slot == Some(i as u8) { "ON" } else { "" };
                    let truncated: String = s.shader.chars().take(28).collect();
                    format!("{:^6} {:<28} {:<5}", i, truncated, active_marker)
                }
            };
            grid.write_row(row_idx, &line);
            if i == self.selected as usize {
                grid.invert_row(row_idx);
            }
        }
    }

    fn handle(&mut self, action: Action, state: &mut SharedState) -> ScreenResult {
        match action {
            Action::NavUp => self.selected = self.selected.saturating_sub(1),
            Action::NavDown => self.selected = (self.selected + 1).min(9),
            Action::Enter => {
                // Enter on a filled slot ⇒ return TriggerShaderSlot so the
                // pipeline activates (not just state.shader_active_slot).
                // Mapping happens via SelectShaderSlot from SHADERS browser.
                let n = self.selected as usize;
                if state.current_shader_bank().slots.get(n).and_then(|o| o.as_ref()).is_some() {
                    return ScreenResult::Action(Action::TriggerShaderSlot(n as u8));
                }
            }
            _ => {}
        }
        ScreenResult::Continue
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shader::ShaderSlot;

    #[test]
    fn nav_clamps_to_zero_and_nine() {
        let mut b = ShdrBnkBody::new();
        let mut s = SharedState::new();
        b.handle(Action::NavUp, &mut s);
        assert_eq!(b.selected, 0);
        for _ in 0..20 {
            b.handle(Action::NavDown, &mut s);
        }
        assert_eq!(b.selected, 9);
    }

    #[test]
    fn enter_on_filled_slot_returns_trigger_action() {
        let mut s = SharedState::new();
        s.current_shader_bank_mut().slots[3] = Some(ShaderSlot {
            shader: "color_shift".into(),
            params: [0.0; 8],
        });
        let mut b = ShdrBnkBody::new();
        for _ in 0..3 { b.handle(Action::NavDown, &mut s); }
        let result = b.handle(Action::Enter, &mut s);
        match result {
            ScreenResult::Action(Action::TriggerShaderSlot(3)) => (),
            _ => panic!("expected ScreenResult::Action(TriggerShaderSlot(3))"),
        }
    }

    #[test]
    fn enter_on_empty_slot_is_noop() {
        let mut s = SharedState::new();
        let mut b = ShdrBnkBody::new();
        b.handle(Action::Enter, &mut s);
        assert_eq!(s.shader_active_slot, None);
    }
}
