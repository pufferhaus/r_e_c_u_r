//! Video render — desktop backend gated by `desktop` feature.

#[cfg(feature = "desktop")]
mod desktop;
mod shader;

#[cfg(feature = "desktop")]
pub use desktop::WinitGlTarget as Render;

// Fallback stub for builds with neither `desktop` nor `pi`.
// Lets unit tests compile in headless CI without window-system deps.
#[cfg(not(feature = "desktop"))]
mod stub {
    pub struct Render;
    impl Render {
        pub fn new(_w: u32, _h: u32, _t: &str) -> anyhow::Result<Self> {
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
        pub fn end_frame(&mut self) {}
    }
}
#[cfg(not(feature = "desktop"))]
pub use stub::Render;
