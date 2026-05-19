//! Shader metadata parser. Mirrors mandleROT's `SceneMeta` with:
//! - `min_gles` instead of `min_pi_gen`
//! - param slot range 0..=7 (not 0..=8)
//! - audio fields parsed but ignored until recur ships audio capture.

use serde::Deserialize;

use crate::error::{Error, Result};

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
pub enum GlesVersion {
    #[default]
    #[serde(rename = "1.00")]
    V100,
    #[serde(rename = "3.10")]
    V310,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Curve {
    Linear,
    Exp,
    Log,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AudioRoute {
    #[default]
    None,
    Bass,
    Lomid,
    Himid,
    Treble,
    Beat,
    Mid,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ParamDef {
    pub slot: u8,
    pub name: String,
    pub min: f32,
    pub max: f32,
    pub default: f32,
    #[serde(default = "default_curve")]
    pub curve: Curve,
    #[serde(default)]
    pub audio_route: AudioRoute,
    #[serde(default)]
    pub audio_amount: f32,
    #[serde(default = "default_polarity")]
    pub audio_polarity: f32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ShaderMeta {
    pub name: String,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub min_gles: GlesVersion,
    #[serde(default)]
    pub internal_resolution: Option<String>,
    #[serde(default)]
    pub params: Vec<ParamDef>,
}

fn default_curve() -> Curve {
    Curve::Linear
}
fn default_polarity() -> f32 {
    1.0
}

impl ShaderMeta {
    pub fn parse(s: &str, file_label: &str) -> Result<Self> {
        toml::from_str(s).map_err(|e| Error::ShaderMeta {
            file: file_label.to_string(),
            source: e,
        })
    }

    /// Validate cross-field constraints (slot range, slot uniqueness, range sanity).
    pub fn validate(&self) -> Result<()> {
        let mut seen = [false; 8];
        for p in &self.params {
            if p.slot > 7 {
                return Err(Error::ShaderCompile(format!(
                    "param slot {} out of range (must be 0-7)",
                    p.slot
                )));
            }
            if seen[p.slot as usize] {
                return Err(Error::ShaderCompile(format!(
                    "duplicate param slot {}",
                    p.slot
                )));
            }
            seen[p.slot as usize] = true;
            if p.min >= p.max {
                return Err(Error::ShaderCompile(format!(
                    "param {} has min >= max",
                    p.name
                )));
            }
            if p.default < p.min || p.default > p.max {
                return Err(Error::ShaderCompile(format!(
                    "param {} default outside [min, max]",
                    p.name
                )));
            }
        }
        Ok(())
    }

    /// Parse the `internal_resolution = "WxH"` string into pixel dims, if set.
    pub fn internal_resolution_size(&self) -> Option<(u32, u32)> {
        let s = self.internal_resolution.as_ref()?;
        let mut parts = s.split('x');
        let w: u32 = parts.next()?.trim().parse().ok()?;
        let h: u32 = parts.next()?.trim().parse().ok()?;
        if w == 0 || h == 0 {
            return None;
        }
        Some((w, h))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn good() -> &'static str {
        include_str!("../../tests/fixtures/shader_good.toml")
    }

    fn bad() -> &'static str {
        include_str!("../../tests/fixtures/shader_bad.toml")
    }

    #[test]
    fn parses_good_shader() {
        let m = ShaderMeta::parse(good(), "shader_good.toml").unwrap();
        assert_eq!(m.name, "test_shader");
        assert_eq!(m.display_name.as_deref(), Some("Test"));
        assert_eq!(m.min_gles, GlesVersion::V100);
        assert_eq!(m.params.len(), 2);
        assert_eq!(m.params[0].name, "hue");
        assert_eq!(m.params[0].audio_route, AudioRoute::Bass);
        assert_eq!(m.params[1].curve, Curve::Exp);
        assert!(m.validate().is_ok());
    }

    #[test]
    fn rejects_bad_toml() {
        let err = ShaderMeta::parse(bad(), "shader_bad.toml").unwrap_err();
        assert!(matches!(err, Error::ShaderMeta { .. }));
    }

    #[test]
    fn validate_catches_out_of_range_slot() {
        let s = r#"
            name = "x"
            [[params]]
            slot = 8
            name = "a"
            min = 0.0
            max = 1.0
            default = 0.5
        "#;
        let m = ShaderMeta::parse(s, "x").unwrap();
        let err = m.validate().unwrap_err();
        assert!(err.to_string().contains("out of range"));
    }

    #[test]
    fn validate_catches_duplicate_slot() {
        let s = r#"
            name = "x"
            [[params]]
            slot = 0
            name = "a"
            min = 0.0
            max = 1.0
            default = 0.5
            [[params]]
            slot = 0
            name = "b"
            min = 0.0
            max = 1.0
            default = 0.5
        "#;
        let m = ShaderMeta::parse(s, "x").unwrap();
        let err = m.validate().unwrap_err();
        assert!(err.to_string().contains("duplicate param slot 0"));
    }

    #[test]
    fn min_gles_defaults_to_1_00() {
        let s = "name = \"x\"\n";
        let m = ShaderMeta::parse(s, "x").unwrap();
        assert_eq!(m.min_gles, GlesVersion::V100);
    }

    #[test]
    fn parses_min_gles_3_10() {
        let s = "name = \"x\"\nmin_gles = \"3.10\"\n";
        let m = ShaderMeta::parse(s, "x").unwrap();
        assert_eq!(m.min_gles, GlesVersion::V310);
    }

    #[test]
    fn internal_resolution_parses_wxh() {
        let s = "name = \"x\"\ninternal_resolution = \"720x480\"\n";
        let m = ShaderMeta::parse(s, "x").unwrap();
        assert_eq!(m.internal_resolution_size(), Some((720, 480)));
    }
}
