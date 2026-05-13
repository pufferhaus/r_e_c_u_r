//! BrowserBody — file tree walker, slot mapping via Function key.

use std::collections::HashSet;

use crate::action::Action;
use crate::sample::browser_walk::{walk_browser, BrowserRow};
use crate::state::{SharedState, Slot};
use crate::status::grid::TextGrid;
use crate::ui::{Screen, ScreenResult};

pub struct BrowserBody {
    open: HashSet<std::path::PathBuf>,
    selected: usize,
    top: usize,
}

const VIEW_ROWS: usize = 10;

impl BrowserBody {
    pub fn new() -> Self {
        Self {
            open: HashSet::new(),
            selected: 0,
            top: 0,
        }
    }

    fn rows(&self, state: &SharedState) -> Vec<BrowserRow> {
        walk_browser(&state.paths_to_browser, &self.open)
    }
}

impl Screen for BrowserBody {
    fn render(&self, state: &SharedState, grid: &mut TextGrid) {
        let rows = self.rows(state);
        grid.write_row(4, "path                                slot");
        for view_i in 0..VIEW_ROWS {
            let row_idx = 5 + view_i;
            let abs = self.top + view_i;
            if abs >= rows.len() {
                grid.write_row(row_idx, "");
                continue;
            }
            let row = &rows[abs];
            let slot = slot_label_for(state, &row.path).unwrap_or_else(|| {
                if row.is_file { "-" } else { "x" }.to_string()
            });
            let truncated: String = row.display.chars().take(38).collect();
            grid.write_row(row_idx, &format!("{:<38} {:<5}", truncated, slot));
            if abs == self.selected {
                grid.invert_row(row_idx);
            }
        }
    }

    fn handle(&mut self, action: Action, state: &mut SharedState) -> ScreenResult {
        let rows = self.rows(state);
        let n = rows.len();
        if n == 0 {
            return ScreenResult::Continue;
        }
        match action {
            Action::NavUp => {
                self.selected = self.selected.saturating_sub(1);
                if self.selected < self.top {
                    self.top = self.selected;
                }
            }
            Action::NavDown => {
                self.selected = (self.selected + 1).min(n - 1);
                if self.selected >= self.top + VIEW_ROWS {
                    self.top = self.selected + 1 - VIEW_ROWS;
                }
            }
            Action::Enter => {
                let row = rows[self.selected].clone();
                if row.is_file {
                    if let Some(idx) = state.current_bank().first_empty() {
                        let slot = Slot {
                            location: row.path.clone(),
                            name: row.path.file_name().unwrap().to_string_lossy().into_owned(),
                            start: -1.0,
                            end: -1.0,
                            length: 0.0,
                            rate: 1.0,
                        };
                        state.current_bank_mut().slots[idx] = Some(slot);
                    }
                } else if self.open.contains(&row.path) {
                    self.open.remove(&row.path);
                } else {
                    self.open.insert(row.path.clone());
                }
            }
            _ => {}
        }
        ScreenResult::Continue
    }
}

fn slot_label_for(state: &SharedState, path: &std::path::Path) -> Option<String> {
    for (b_idx, bank) in state.banks.iter().enumerate() {
        for (s_idx, slot) in bank.slots.iter().enumerate() {
            if let Some(s) = slot {
                if s.location == path {
                    return Some(format!("{}-{}", b_idx, s_idx));
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn enter_on_file_adds_to_first_empty_slot() {
        let tmp = tempfile::tempdir().unwrap();
        let f = tmp.path().join("v.mp4");
        fs::write(&f, b"").unwrap();
        let mut st = SharedState::new();
        st.paths_to_browser = vec![tmp.path().to_path_buf()];
        let mut b = BrowserBody::new();
        b.handle(Action::Enter, &mut st);
        let slot = st.banks[0].slots[0].as_ref().unwrap();
        assert_eq!(slot.name, "v.mp4");
    }

    #[test]
    fn enter_on_folder_toggles_open_state() {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir(tmp.path().join("d")).unwrap();
        let mut st = SharedState::new();
        st.paths_to_browser = vec![tmp.path().to_path_buf()];
        let mut b = BrowserBody::new();
        b.handle(Action::Enter, &mut st);
        assert_eq!(b.open.len(), 1);
        b.handle(Action::Enter, &mut st);
        assert_eq!(b.open.len(), 0);
    }

    #[test]
    fn render_handles_non_ascii_display_name() {
        let tmp = tempfile::tempdir().unwrap();
        // Create a file with a non-ASCII name longer than 38 chars.
        let name = "日本語ファイル名前テスト動画クリップ長いファイル名.mp4";
        fs::write(tmp.path().join(name), b"").unwrap();
        let mut st = SharedState::new();
        st.paths_to_browser = vec![tmp.path().to_path_buf()];
        let b = BrowserBody::new();
        let mut grid = crate::status::grid::TextGrid::new(48, 17);
        // Should not panic.
        b.render(&st, &mut grid);
    }
}
