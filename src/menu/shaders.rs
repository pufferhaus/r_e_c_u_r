//! ShadersBody — browse paired `.glsl + .toml` pairs from the shader dir.
//! Enter on a row stashes the shader name into `state.shader_pending_select`;
//! the user then presses Function + SelectShaderSlot(n) to map it.

use crate::action::Action;
use crate::state::SharedState;
use crate::status::grid::TextGrid;
use crate::ui::{Screen, ScreenResult};

pub struct ShadersBody {
    pub names: Vec<String>,
    pub filtered: usize,
    selected: usize,
}

impl ShadersBody {
    pub fn new(names: Vec<String>, filtered: usize) -> Self {
        Self { names, filtered, selected: 0 }
    }
}

impl Screen for ShadersBody {
    fn render(&self, _state: &SharedState, grid: &mut TextGrid) {
        grid.write_row(4, "shader                              gles");
        for view_i in 0..10 {
            let row_idx = 5 + view_i;
            match self.names.get(view_i) {
                None => grid.write_row(row_idx, ""),
                Some(name) => {
                    let truncated: String = name.chars().take(38).collect();
                    grid.write_row(row_idx, &format!("{:<38} {:<5}", truncated, ""));
                    if view_i == self.selected {
                        grid.invert_row(row_idx);
                    }
                }
            }
        }
        let footer = if self.filtered > 0 {
            format!("{} shown, {} hidden (pi5-only)", self.names.len(), self.filtered)
        } else {
            format!("{} shaders", self.names.len())
        };
        grid.write_row(14, &footer);
    }

    fn handle(&mut self, action: Action, state: &mut SharedState) -> ScreenResult {
        if self.names.is_empty() {
            return ScreenResult::Continue;
        }
        match action {
            Action::NavUp => self.selected = self.selected.saturating_sub(1),
            Action::NavDown => self.selected = (self.selected + 1).min(self.names.len() - 1),
            Action::Enter => {
                state.shader_pending_select = Some(self.names[self.selected].clone());
            }
            _ => {}
        }
        ScreenResult::Continue
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::Action;
    use crate::state::SharedState;

    #[test]
    fn enter_stashes_selected_name_into_state() {
        let names = vec!["color_shift".to_string(), "pixelate".to_string()];
        let mut body = ShadersBody::new(names, 0);
        let mut s = SharedState::new();
        body.handle(Action::NavDown, &mut s);
        body.handle(Action::Enter, &mut s);
        assert_eq!(s.shader_pending_select.as_deref(), Some("pixelate"));
    }

    #[test]
    fn footer_shows_filtered_count_when_nonzero() {
        let body = ShadersBody::new(vec!["a".into()], 3);
        let mut grid = crate::status::grid::TextGrid::new(48, 17);
        let s = SharedState::new();
        body.render(&s, &mut grid);
        // Row 14 should contain "hidden".
        let row14: String = (0..48).map(|c| grid.at(14, c).ch).collect();
        assert!(row14.contains("hidden"));
    }
}
