//! BrowserBody — file tree walker, slot mapping via Function key.

use std::collections::HashSet;

use crate::action::Action;
use crate::sample::browser_walk::{walk_browser, BrowserRow};
use crate::state::{SharedState, Slot, SourceKind};
use crate::status::grid::TextGrid;
use crate::ui::{Screen, ScreenResult};
use crate::video::{CodecStatus, ProbeRequest};

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

    /// If the currently focused row is a file with no cached probe entry,
    /// enqueue a probe request via state.probe_tx. No-op when no tx wired,
    /// when the cursor is on a folder, or when the cache already has an entry.
    fn maybe_enqueue_probe(&self, state: &mut SharedState, rows: &[BrowserRow]) {
        let Some(row) = rows.get(self.selected) else { return; };
        if !row.is_file {
            return;
        }
        let mtime = std::fs::metadata(&row.path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        if state
            .probe_cache
            .get_with_mtime(&row.probe_key, mtime)
            .is_some()
        {
            return;
        }
        if let Some(tx) = &state.probe_tx {
            let req = ProbeRequest {
                path: row.probe_key.clone(),
                mtime,
            };
            state.probe_cache.mark_pending(&row.probe_key, mtime);
            let _ = tx.send(req);
        }
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
            let cached = state.probe_cache.get(&row.probe_key);
            // Probe-status marker appended after the slot column.
            let marker = match &cached {
                Some(CodecStatus::Pending) => " […]",
                Some(CodecStatus::Unsupported(_)) => " [X]",
                _ => "",
            };
            grid.write_row(row_idx, &format!("{:<38} {:<5}{}", truncated, slot, marker));
            if matches!(cached, Some(CodecStatus::Unsupported(_))) {
                grid.dim_row(row_idx);
            }
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
                    // Refuse mapping if the cached probe says unsupported.
                    if let Some(CodecStatus::Unsupported(codec)) =
                        state.probe_cache.get(&row.probe_key)
                    {
                        let profile_label = match state.gles_profile {
                            crate::render::shader_assembly::GlesProfile::V100 => "pi3",
                            crate::render::shader_assembly::GlesProfile::V310 => "pi5",
                        };
                        state.last_error = Some(format!(
                            "cannot map: {profile_label} build does not support {codec}"
                        ));
                    } else if let Some(idx) = state.current_bank().first_empty() {
                        let slot = Slot {
                            source: SourceKind::File(row.path.clone()),
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
        // Lazy probe enqueue: any handled action triggers a check, so the very
        // first action the user takes (whether navigation, Enter, or a no-op key)
        // kicks off the probe for the focused row.
        self.maybe_enqueue_probe(state, &rows);
        ScreenResult::Continue
    }
}

fn slot_label_for(state: &SharedState, path: &std::path::Path) -> Option<String> {
    for (b_idx, bank) in state.banks.iter().enumerate() {
        for (s_idx, slot) in bank.slots.iter().enumerate() {
            if let Some(s) = slot {
                if s.file_path() == Some(path) {
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
    fn render_dims_unsupported_codec_row() {
        use crate::video::CodecStatus;
        let tmp = tempfile::tempdir().unwrap();
        let f = tmp.path().join("a.mp4");
        fs::write(&f, b"").unwrap();
        let canon = std::fs::canonicalize(&f).unwrap();

        let mut st = SharedState::new();
        st.paths_to_browser = vec![tmp.path().to_path_buf()];
        st.probe_cache.insert(&canon, 0, CodecStatus::Unsupported("hevc".into()));

        let b = BrowserBody::new();
        let mut grid = crate::status::grid::TextGrid::new(48, 17);
        b.render(&st, &mut grid);

        // Row 5 is the first body row. The unsupported file should be dimmed.
        let row5_attr = grid.at(5, 0).attr;
        assert!(row5_attr & crate::status::grid::ATTR_DIM != 0,
            "row 5 should have ATTR_DIM (got attr={row5_attr:#04x})");
        let row5: String = (0..48).map(|c| grid.at(5, c).ch).collect();
        assert!(row5.contains("[X]"), "row should contain [X] marker, got: {row5:?}");
    }

    #[test]
    fn render_shows_pending_glyph_during_probe() {
        use crate::video::CodecStatus;
        let tmp = tempfile::tempdir().unwrap();
        let f = tmp.path().join("a.mp4");
        fs::write(&f, b"").unwrap();
        let canon = std::fs::canonicalize(&f).unwrap();

        let mut st = SharedState::new();
        st.paths_to_browser = vec![tmp.path().to_path_buf()];
        st.probe_cache.insert(&canon, 0, CodecStatus::Pending);

        let b = BrowserBody::new();
        let mut grid = crate::status::grid::TextGrid::new(48, 17);
        b.render(&st, &mut grid);

        let row5: String = (0..48).map(|c| grid.at(5, c).ch).collect();
        assert!(row5.contains("[..]") || row5.contains("[…]"), "expected pending marker, got: {row5:?}");
    }

    #[test]
    fn enter_on_unsupported_file_sets_status_line_and_does_not_map() {
        use crate::video::CodecStatus;
        let tmp = tempfile::tempdir().unwrap();
        let f = tmp.path().join("hevc_clip.mp4");
        fs::write(&f, b"").unwrap();
        let canon = std::fs::canonicalize(&f).unwrap();

        let mut st = SharedState::new();
        st.paths_to_browser = vec![tmp.path().to_path_buf()];
        st.probe_cache.insert(&canon, 0, CodecStatus::Unsupported("hevc".into()));

        let mut b = BrowserBody::new();
        b.handle(Action::Enter, &mut st);

        assert!(st.banks[0].slots[0].is_none(), "slot 0 should stay empty");
        let err = st.last_error.as_deref().unwrap_or("");
        assert!(err.contains("hevc"), "got: {err:?}");
        assert!(err.contains("cannot map"), "got: {err:?}");
    }

    #[test]
    fn any_action_enqueues_probe_for_focused_file() {
        use crate::video::ProbeRequest;
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("a.mp4"), b"").unwrap();

        let (tx, rx) = crossbeam_channel::unbounded::<ProbeRequest>();
        let mut st = SharedState::new();
        st.paths_to_browser = vec![tmp.path().to_path_buf()];
        st.probe_tx = Some(tx);

        let mut b = BrowserBody::new();
        // First action the user takes — even Back, not nav — triggers a probe.
        b.handle(Action::Back, &mut st);
        assert!(rx.try_recv().is_ok(), "first action should enqueue probe for focused row");
    }

    #[test]
    fn nav_to_file_enqueues_probe_request_when_tx_set() {
        use crate::video::ProbeRequest;
        let tmp = tempfile::tempdir().unwrap();
        let f = tmp.path().join("a.mp4");
        fs::write(&f, b"").unwrap();
        let canon = std::fs::canonicalize(&f).unwrap();

        let (tx, rx) = crossbeam_channel::unbounded::<ProbeRequest>();
        let mut st = SharedState::new();
        st.paths_to_browser = vec![tmp.path().to_path_buf()];
        st.probe_tx = Some(tx);

        let mut b = BrowserBody::new();
        // Trigger a nav action; the focused row (a.mp4) has no cache entry, so a probe should enqueue.
        b.handle(Action::NavUp, &mut st);

        let req = rx.try_recv().expect("expected probe request");
        assert_eq!(req.path, canon);
    }

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
