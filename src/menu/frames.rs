//! FRAMES body — ring stats + scrub readouts. Reads everything from
//! `SharedState.detour` + `SharedState.frames_stats_*`.

use crate::action::Action;
use crate::state::SharedState;
use crate::status::grid::TextGrid;
use crate::ui::{Screen, ScreenResult};

pub struct FramesBody;

impl FramesBody {
    pub fn new() -> Self {
        Self
    }
}

impl Default for FramesBody {
    fn default() -> Self {
        Self::new()
    }
}

impl Screen for FramesBody {
    fn render(&self, state: &SharedState, grid: &mut TextGrid) {
        let d = &state.detour;
        let fps = state.frames_stats_fps.max(1);
        let scrub_age_s = (state
            .frames_stats_count
            .saturating_sub(1)
            .saturating_sub(d.read_position) as f32)
            / fps as f32;
        let marker_str = match (d.start_marker, d.end_marker) {
            (Some(a), Some(b)) => format!("[{a}..{b}]"),
            (Some(a), None) => format!("[{a}..]"),
            (None, Some(b)) => format!("[..{b}]"),
            _ => "[none]".to_string(),
        };
        grid.write_row(
            5,
            &format!(
                "ring: {}/{} frames ({}/{} MB)",
                state.frames_stats_count,
                state.frames_stats_capacity,
                state.frames_stats_used_mb,
                state.frames_stats_budget_mb
            ),
        );
        grid.write_row(
            6,
            &format!(
                "scrub: frame {}/{} ({:.2}s ago)",
                d.read_position,
                state.frames_stats_count.saturating_sub(1),
                scrub_age_s
            ),
        );
        grid.write_row(
            7,
            &format!(
                "speed: {:.2}x  dir: {}  mix: {:.0}%",
                d.speed,
                if d.forward { "fwd" } else { "rev" },
                d.mix * 100.0
            ),
        );
        grid.write_row(8, &format!("markers: {}", marker_str));
        grid.write_row(
            9,
            &format!("auto-play: {}", if d.auto_play { "ON" } else { "off" }),
        );
    }

    fn handle(&mut self, _action: Action, _state: &mut SharedState) -> ScreenResult {
        // All scrub keys go through apply.rs (via keymap → Action::Detour*).
        ScreenResult::Continue
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_shows_ring_count_and_capacity() {
        let body = FramesBody::new();
        let mut st = SharedState::new();
        st.frames_stats_count = 34;
        st.frames_stats_capacity = 100;
        st.frames_stats_used_mb = 50;
        st.frames_stats_budget_mb = 128;
        let mut grid = TextGrid::new(48, 17);
        body.render(&st, &mut grid);
        let row5: String = (0..48).map(|c| grid.at(5, c).ch).collect();
        assert!(row5.contains("34/100"), "got: {row5}");
        assert!(row5.contains("50/128"), "got: {row5}");
    }

    #[test]
    fn render_shows_auto_play_state() {
        let body = FramesBody::new();
        let mut st = SharedState::new();
        st.detour.auto_play = true;
        let mut grid = TextGrid::new(48, 17);
        body.render(&st, &mut grid);
        let row9: String = (0..48).map(|c| grid.at(9, c).ch).collect();
        assert!(row9.contains("ON"));
    }

    #[test]
    fn render_shows_speed_and_direction() {
        let body = FramesBody::new();
        let mut st = SharedState::new();
        st.detour.speed = 2.0;
        st.detour.forward = false;
        st.detour.mix = 0.75;
        let mut grid = TextGrid::new(48, 17);
        body.render(&st, &mut grid);
        let row7: String = (0..48).map(|c| grid.at(7, c).ch).collect();
        assert!(row7.contains("2.00x"), "got: {row7}");
        assert!(row7.contains("rev"), "got: {row7}");
        assert!(row7.contains("75%"), "got: {row7}");
    }
}
