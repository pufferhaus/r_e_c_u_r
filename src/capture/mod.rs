//! Live capture sources (Phase 4 — captur).

pub mod device;
pub use device::{enumerate_capture_devices, CaptureDevice};

pub mod recording;
