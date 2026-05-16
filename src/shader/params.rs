//! Per-shader runtime parameter values. One `ParamMap` per active shader
//! tracks the live value of each of the 8 `u_param0..u_param7` slots.
//!
//! Audio routing is parsed from `ShaderMeta` but applied as a no-op here
//! until recur ships audio capture (see backlog).

use super::meta::{ParamDef, ShaderMeta};

#[derive(Debug, Clone)]
pub struct ParamMap {
    /// Current value per slot, indexed 0..=7.
    values: [f32; 8],
    /// Cached metadata per slot (None if no param assigned to that slot).
    defs: [Option<ParamDef>; 8],
}

impl ParamMap {
    /// Build a fresh map seeded with each `ParamDef`'s default value.
    pub fn from_meta(meta: &ShaderMeta) -> Self {
        let mut values = [0.0_f32; 8];
        let mut defs: [Option<ParamDef>; 8] = Default::default();
        for p in &meta.params {
            let idx = p.slot as usize;
            if idx >= 8 {
                continue;
            }
            values[idx] = p.default;
            defs[idx] = Some(p.clone());
        }
        Self { values, defs }
    }

    /// Read the current value at slot 0..=7.
    pub fn get(&self, slot: usize) -> f32 {
        self.values.get(slot).copied().unwrap_or(0.0)
    }

    /// Write a value at slot 0..=7, clamped to the param's [min, max] if
    /// metadata is known. Out-of-range slots silently no-op.
    pub fn set(&mut self, slot: usize, value: f32) {
        if slot >= 8 {
            return;
        }
        let clamped = match &self.defs[slot] {
            Some(d) => value.clamp(d.min, d.max),
            None => value,
        };
        self.values[slot] = clamped;
    }

    /// Read all 8 values as a flat array (suitable for uploading to
    /// `u_param0`..`u_param7` uniforms).
    pub fn as_array(&self) -> [f32; 8] {
        self.values
    }

    /// Return the param definition for slot 0..=7, if any.
    pub fn def(&self, slot: usize) -> Option<&ParamDef> {
        self.defs.get(slot).and_then(|o| o.as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta_with_defaults() -> ShaderMeta {
        let s = r#"
            name = "x"
            [[params]]
            slot = 0
            name = "a"
            min = -1.0
            max = 1.0
            default = 0.25
            [[params]]
            slot = 3
            name = "b"
            min = 0.0
            max = 10.0
            default = 5.0
        "#;
        ShaderMeta::parse(s, "x").unwrap()
    }

    #[test]
    fn from_meta_seeds_defaults() {
        let m = ParamMap::from_meta(&meta_with_defaults());
        assert_eq!(m.get(0), 0.25);
        assert_eq!(m.get(3), 5.0);
        assert_eq!(m.get(1), 0.0);
        assert_eq!(m.get(7), 0.0);
    }

    #[test]
    fn set_clamps_to_meta_range() {
        let mut m = ParamMap::from_meta(&meta_with_defaults());
        m.set(0, 5.0);
        assert_eq!(m.get(0), 1.0);
        m.set(0, -10.0);
        assert_eq!(m.get(0), -1.0);
    }

    #[test]
    fn set_without_def_does_not_clamp() {
        let mut m = ParamMap::from_meta(&meta_with_defaults());
        m.set(1, 999.0);
        assert_eq!(m.get(1), 999.0);
    }

    #[test]
    fn set_out_of_range_slot_noops() {
        let mut m = ParamMap::from_meta(&meta_with_defaults());
        m.set(99, 1.0);
        assert_eq!(m.as_array(), [0.25, 0.0, 0.0, 5.0, 0.0, 0.0, 0.0, 0.0]);
    }
}
