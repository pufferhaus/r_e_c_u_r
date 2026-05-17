//! Phase 3 — detour (in-memory frame ring + scrub mode).

pub mod ring;
pub use ring::Ring;

pub mod budget;
pub use budget::{default_budget_mb_for_build, resolved_budget_bytes};

pub mod settings;
pub use settings::DetourSettings;
