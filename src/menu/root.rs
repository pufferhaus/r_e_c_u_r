//! Always-on chrome screen. Renders the title, transport banner, status row,
//! mode tabs, column headers, and footer. Delegates the body region to one
//! of BrowserBody / SamplerBody / SettingsBody / ShadersBody / ShdrBnkBody.

use crate::action::Action;
use crate::render::shader_assembly::GlesProfile;
use crate::state::{ControlMode, DisplayMode, SharedState};
use crate::status::grid::TextGrid;
use crate::ui::{Screen, ScreenResult};

use super::{
    browser::BrowserBody, frames::FramesBody, param::ParamBody, sampler::SamplerBody,
    settings::SettingsBody, shaders::ShadersBody, shdr_bnk::ShdrBnkBody,
};

pub struct RootScreen {
    browser: BrowserBody,
    frames: FramesBody,
    sampler: SamplerBody,
    settings: SettingsBody,
    shaders: ShadersBody,
    shdr_bnk: ShdrBnkBody,
    param: ParamBody,
}

impl RootScreen {
    pub fn new() -> Self {
        Self {
            browser: BrowserBody::new(),
            frames: FramesBody::new(),
            sampler: SamplerBody::new(),
            settings: SettingsBody::new(),
            shaders: ShadersBody::new(Vec::new(), 0),
            shdr_bnk: ShdrBnkBody::new(),
            param: ParamBody::new(),
        }
    }

    /// Refresh the SHADERS browser list after a shader library reload.
    /// Called from `main.rs` via Task 16; method is wired here in Task 10.
    pub fn set_shader_names(&mut self, names: Vec<String>, filtered: usize) {
        self.shaders = ShadersBody::new(names, filtered);
    }

    fn render_chrome(&self, state: &SharedState, grid: &mut TextGrid) {
        // Row 1 — title
        let title = match state.display_mode {
            DisplayMode::Shaders | DisplayMode::ShdrBnk => {
                "============== c_o_n_j_u_r ============="
            }
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
        let mut footer = if let Some(err) = state.last_error.as_deref() {
            format!("ERR: {}", err.chars().take(40).collect::<String>())
        } else if state.function_on {
            "               < FUNCTION KEY ON >".to_string()
        } else {
            format!("CONTROL: {:?}", state.control_mode)
        };
        if state.gles_profile == GlesProfile::V100 {
            footer.push_str(" [profile: pi3]");
        }
        // Phase 4b — recording indicator.
        if let Some(rec) = state.active_recording.as_ref() {
            use crate::capture::recording::RecState;
            let suffix = match rec.state {
                RecState::Recording => {
                    let elapsed = rec.started_at.elapsed();
                    let secs = elapsed.as_secs();
                    format!(" <REC> {:02}:{:02}", secs / 60, secs % 60)
                }
                RecState::Finalizing => " <SAV>".to_string(),
            };
            // Right-trim the footer to make room.
            let max_w = 40usize.saturating_sub(suffix.chars().count());
            footer = footer.chars().take(max_w).collect::<String>() + &suffix;
        }
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
            DisplayMode::Shaders => self.shaders.render(state, grid),
            DisplayMode::ShdrBnk => self.shdr_bnk.render(state, grid),
            DisplayMode::Frames => self.frames.render(state, grid),
        }
        if state.control_mode == ControlMode::ShaderParam {
            self.param.render(state, grid);
        }
    }

    fn handle(&mut self, action: Action, state: &mut SharedState) -> ScreenResult {
        if state.control_mode == ControlMode::ShaderParam {
            return self.param.handle(action, state);
        }
        match state.display_mode {
            DisplayMode::Browser => self.browser.handle(action, state),
            DisplayMode::Sampler => self.sampler.handle(action, state),
            DisplayMode::Settings => self.settings.handle(action, state),
            DisplayMode::Shaders => self.shaders.handle(action, state),
            DisplayMode::ShdrBnk => self.shdr_bnk.handle(action, state),
            DisplayMode::Frames => self.frames.handle(action, state),
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
    fn footer_shows_gles_profile_indicator_when_v100() {
        use crate::render::shader_assembly::GlesProfile;
        let mut st = SharedState::new();
        st.gles_profile = GlesProfile::V100;
        st.display_mode = DisplayMode::Sampler;
        let root = RootScreen::new();
        let mut grid = crate::status::grid::TextGrid::new(48, 17);
        root.render(&st, &mut grid);
        let row15: String = (0..48).map(|c| grid.at(15, c).ch).collect();
        assert!(
            row15.contains("profile: pi3") || row15.contains("v100"),
            "footer should call out pi3 compat mode, got: {row15}"
        );
    }

    #[test]
    fn shdr_bnk_mode_renders_shdr_bnk_body() {
        use crate::shader::ShaderSlot;
        let mut st = SharedState::new();
        st.display_mode = DisplayMode::ShdrBnk;
        st.current_shader_bank_mut().slots[2] = Some(ShaderSlot {
            shader: "kaleidoscope".into(),
            params: [0.0; 8],
        });
        let root = RootScreen::new();
        let mut grid = crate::status::grid::TextGrid::new(48, 17);
        root.render(&st, &mut grid);
        // Row 7 (slot 2) should contain the shader name.
        let row7: String = (0..48).map(|c| grid.at(7, c).ch).collect();
        assert!(row7.contains("kaleidoscope"), "got: {row7}");
    }

    #[test]
    fn footer_shows_rec_when_active_recording() {
        use crate::capture::recording::{ActiveRecording, RecState};
        use std::time::Instant;
        let mut s = SharedState::new();
        s.active_recording = Some(ActiveRecording {
            device_path: "/dev/video0".into(),
            file_path: "/tmp/rec.mp4".into(),
            started_at: Instant::now(),
            state: RecState::Recording,
            last_disk_check: Instant::now(),
        });
        let r = RootScreen::new();
        let mut grid = TextGrid::new(48, 17);
        r.render_chrome(&s, &mut grid);
        let footer = grid.row_text(15);
        assert!(footer.contains("<REC>"), "footer: {footer:?}");
    }

    #[test]
    fn footer_shows_sav_when_finalizing() {
        use crate::capture::recording::{ActiveRecording, RecState};
        use std::time::Instant;
        let mut s = SharedState::new();
        s.active_recording = Some(ActiveRecording {
            device_path: "/dev/video0".into(),
            file_path: "/tmp/rec.mp4".into(),
            started_at: Instant::now(),
            state: RecState::Finalizing,
            last_disk_check: Instant::now(),
        });
        let r = RootScreen::new();
        let mut grid = TextGrid::new(48, 17);
        r.render_chrome(&s, &mut grid);
        let footer = grid.row_text(15);
        assert!(footer.contains("<SAV>"), "footer: {footer:?}");
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
        assert!(
            grid.at(5, 0).attr & ATTR_INVERSE != 0,
            "row 5 should be inverted before nav"
        );
        assert!(
            grid.at(6, 0).attr & ATTR_INVERSE == 0,
            "row 6 should not be inverted before nav"
        );

        // NavDown advances selected to 1; now row 6 should be inverted.
        root.handle(Action::NavDown, &mut st);
        let mut grid2 = TextGrid::new(48, 17);
        root.render(&st, &mut grid2);
        assert!(
            grid2.at(5, 0).attr & ATTR_INVERSE == 0,
            "row 5 should not be inverted after nav"
        );
        assert!(
            grid2.at(6, 0).attr & ATTR_INVERSE != 0,
            "row 6 should be inverted after nav"
        );
    }
}
