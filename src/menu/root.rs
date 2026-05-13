//! Always-on chrome screen. Renders the title, transport banner, status row,
//! mode tabs, column headers, and footer. Delegates the body region to one
//! of BrowserBody / SamplerBody / SettingsBody.

use crate::action::Action;
use crate::state::{DisplayMode, SharedState};
use crate::status::grid::TextGrid;
use crate::ui::{Screen, ScreenResult};

use super::{browser::BrowserBody, sampler::SamplerBody, settings::SettingsBody};

pub struct RootScreen {
    browser: BrowserBody,
    sampler: SamplerBody,
    settings: SettingsBody,
}

impl RootScreen {
    pub fn new() -> Self {
        Self {
            browser: BrowserBody::new(),
            sampler: SamplerBody::new(),
            settings: SettingsBody::new(),
        }
    }

    fn render_chrome(&self, state: &SharedState, grid: &mut TextGrid) {
        // Row 1 — title
        let title = match state.display_mode {
            DisplayMode::Shaders | DisplayMode::ShdrBnk => "============== c_o_n_j_u_r =============",
            DisplayMode::Frames => "============== d_e_t_o_u_r =============",
            _ => "============== r_e_c_u_r ===============",
        };
        grid.write_row(0, title);

        // Row 2 — transport banner (stub when no player loaded)
        grid.write_row(1, " 00:00 [-----------------------] 00:00");

        // Row 3 — status: NOW [b-s] STATUS    NEXT [b-s] STATUS
        grid.write_row(2, "NOW [0-0] -                NEXT [0-0] -");

        // Row 4 — mode tabs
        grid.write_row(3, &body_title(state));

        // Row 16 — footer (status / message)
        let footer = if let Some(err) = state.last_error.as_deref() {
            format!("ERR: {}", err.chars().take(40).collect::<String>())
        } else if state.function_on {
            "               < FUNCTION KEY ON >".to_string()
        } else {
            format!("CONTROL: {:?}", state.control_mode)
        };
        grid.write_row(15, &footer);
    }
}

impl Screen for RootScreen {
    fn render(&self, state: &SharedState, grid: &mut TextGrid) {
        self.render_chrome(state, grid);
        match state.display_mode {
            DisplayMode::Browser => self.browser.render(state, grid),
            DisplayMode::Sampler => self.sampler.render(state, grid),
            DisplayMode::Settings => self.settings.render(state, grid),
            _ => grid.write_row(10, "      (not yet implemented in Phase 1)"),
        }
    }

    fn handle(&mut self, action: Action, state: &mut SharedState) -> ScreenResult {
        match state.display_mode {
            DisplayMode::Browser => self.browser.handle(action, state),
            DisplayMode::Sampler => self.sampler.handle(action, state),
            DisplayMode::Settings => self.settings.handle(action, state),
            _ => ScreenResult::Continue,
        }
    }
}

fn body_title(state: &SharedState) -> String {
    let abbrev = |m: DisplayMode| match m {
        DisplayMode::Browser => "br",
        DisplayMode::Sampler => "sa",
        DisplayMode::Settings => "se",
        DisplayMode::Shaders => "sh",
        DisplayMode::ShdrBnk => "sb",
        DisplayMode::Frames => "fr",
    };
    let all = [
        DisplayMode::Browser,
        DisplayMode::Settings,
        DisplayMode::Sampler,
        DisplayMode::Shaders,
        DisplayMode::ShdrBnk,
        DisplayMode::Frames,
    ];
    let mut parts = Vec::new();
    for m in all {
        if m == state.display_mode {
            parts.push(format!("[{:_<8}]", format!("{:?}", m).to_lowercase()));
        } else {
            parts.push(format!("<{}>", abbrev(m)));
        }
    }
    let s = parts.join("");
    format!("---{}---", &s[..s.len().min(42)])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::status::grid::ATTR_INVERSE;

    #[test]
    fn body_title_brackets_current_mode() {
        let mut st = SharedState::new();
        st.display_mode = DisplayMode::Sampler;
        let t = body_title(&st);
        assert!(t.contains("[sampler"));
    }

    #[test]
    fn nav_routes_to_active_body() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("a.mp4"), b"").unwrap();
        std::fs::write(tmp.path().join("b.mp4"), b"").unwrap();
        let mut st = SharedState::new();
        st.display_mode = DisplayMode::Browser;
        st.paths_to_browser = vec![tmp.path().to_path_buf()];
        let mut root = RootScreen::new();

        // Before NavDown: selected is 0, so row 5 (body row 0) is inverted.
        let mut grid = TextGrid::new(48, 17);
        root.render(&st, &mut grid);
        assert!(grid.at(5, 0).attr & ATTR_INVERSE != 0, "row 5 should be inverted before nav");
        assert!(grid.at(6, 0).attr & ATTR_INVERSE == 0, "row 6 should not be inverted before nav");

        // NavDown advances selected to 1; now row 6 should be inverted.
        root.handle(Action::NavDown, &mut st);
        let mut grid2 = TextGrid::new(48, 17);
        root.render(&st, &mut grid2);
        assert!(grid2.at(5, 0).attr & ATTR_INVERSE == 0, "row 5 should not be inverted after nav");
        assert!(grid2.at(6, 0).attr & ATTR_INVERSE != 0, "row 6 should be inverted after nav");
    }
}
