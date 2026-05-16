//! Phase 2 — shader system (conjur).
//!
//! Vendored from mandleROT's `src/scene/` with recur-specific deltas:
//! - `min_gles` replaces `min_pi_gen`
//! - param slot range 0..=7 (8 params)
//! - audio routing fields parsed but ignored until recur ships audio capture

pub mod meta;
pub mod params;

pub use meta::{AudioRoute, Curve, GlesVersion, ParamDef, ShaderMeta};
pub use params::ParamMap;
