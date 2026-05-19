//! Phase 2 — shader system (conjur).
//!
//! Vendored from mandleROT's `src/scene/` with recur-specific deltas:
//! - `min_gles` replaces `min_pi_gen`
//! - param slot range 0..=7 (8 params)
//! - audio routing fields parsed but ignored until recur ships audio capture

pub mod banks;
pub mod hot_reload;
pub mod library;
pub mod meta;
pub mod params;

pub use banks::{ShaderBank, ShaderSlot, MAX_SHADER_BANKS, SHADER_SLOTS_PER_BANK};
pub use hot_reload::{ShaderEvent, ShaderWatcher};
pub use library::{LoadedShader, ShaderLibrary, SAFE_SHADER};
pub use meta::{AudioRoute, Curve, GlesVersion, ParamDef, ShaderMeta};
pub use params::ParamMap;
