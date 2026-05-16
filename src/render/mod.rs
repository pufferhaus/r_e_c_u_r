//! Video render — desktop backend gated by `desktop` feature, pi by `pi-base`.

#[cfg(feature = "desktop")]
mod desktop;
#[cfg(feature = "pi-base")]
mod pi;
mod shader;
pub mod shader_assembly;
#[cfg(any(feature = "desktop", feature = "pi-base"))]
pub mod shader_pipeline;
#[cfg(any(feature = "desktop", feature = "pi-base"))]
mod text;

#[cfg(feature = "desktop")]
pub use desktop::WinitGlTarget as Render;
#[cfg(all(feature = "pi-base", not(feature = "desktop")))]
pub use pi::PiTarget as Render;

// Headless / no-window fallback when neither feature is enabled.
// Lets unit tests compile in headless CI without window-system deps.
#[cfg(not(any(feature = "desktop", feature = "pi-base")))]
mod stub {
    pub struct Render;
    impl Render {
        pub fn new(_w: u32, _h: u32, _t: &str, _p: crate::render::shader_assembly::GlesProfile) -> anyhow::Result<Self> {
            Ok(Self)
        }
        pub fn pump(&mut self) -> Vec<()> {
            Vec::new()
        }
        pub fn should_close(&self) -> bool {
            false
        }
        pub fn begin_frame(&mut self) {}
        pub fn draw_video_layer(&mut self, _: &[u8], _: u32, _: u32, _: f32) {}
        pub fn draw_text_grid(&mut self, _: &crate::status::grid::TextGrid) {}
        pub fn end_frame(&mut self) {}
        pub fn select_shader(&mut self, _: &str, _: [f32; 8]) -> anyhow::Result<()> { Ok(()) }
        pub fn clear_shader(&mut self) {}
        pub fn pulse_shader_trigger(&mut self) {}
        pub fn invalidate_shader(&mut self, _: &str) {}
        pub fn upsert_shader(&mut self, _: &str, _: crate::shader::LoadedShader) {}
        pub fn set_shader_params(&mut self, _: [f32; 8]) {}
    }
}
#[cfg(not(any(feature = "desktop", feature = "pi-base")))]
pub use stub::Render;
