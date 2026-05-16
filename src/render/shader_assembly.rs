//! Concatenate a fragment-body string with the right GLES prelude so the
//! result can be handed to `glow::Context::shader_source`. Pure-Rust —
//! does not touch GL.

use crate::shader::GlesVersion;

/// The active GLES profile for the current binary or runtime override.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlesProfile {
    V100,
    V310,
}

impl GlesProfile {
    /// Required compile-time profile for this build, per cargo features.
    /// `pi3` → V100. `pi5` → V310. `desktop` → V310 by default; the
    /// `--gles-profile pi3` CLI flag (sub-plan B) overrides to V100.
    pub fn default_for_build() -> Self {
        #[cfg(feature = "pi3")]
        return GlesProfile::V100;
        #[cfg(feature = "pi5")]
        return GlesProfile::V310;
        #[cfg(all(not(feature = "pi3"), not(feature = "pi5")))]
        return GlesProfile::V310;
    }

    pub fn from_meta(min_gles: GlesVersion) -> Self {
        match min_gles {
            GlesVersion::V100 => GlesProfile::V100,
            GlesVersion::V310 => GlesProfile::V310,
        }
    }
}

const PRELUDE_100: &str = include_str!("../../shaders/_prelude_100.glsl");
const PRELUDE_310: &str = include_str!("../../shaders/_prelude_310.glsl");
const VERT_100: &str = include_str!("../../shaders/quad_100.vert");
const VERT_310: &str = include_str!("../../shaders/quad_310.vert");

/// Produce the final fragment-shader source for the given profile.
pub fn assemble_fragment_source(profile: GlesProfile, body: &str) -> String {
    let prelude = match profile {
        GlesProfile::V100 => PRELUDE_100,
        GlesProfile::V310 => PRELUDE_310,
    };
    let mut out = String::with_capacity(prelude.len() + body.len() + 16);
    out.push_str(prelude);
    out.push('\n');
    out.push_str(body);
    out
}

/// Return the vertex-shader source for the given profile.
pub fn vertex_source_for(profile: GlesProfile) -> &'static str {
    match profile {
        GlesProfile::V100 => VERT_100,
        GlesProfile::V310 => VERT_310,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assembly_prepends_v100_prelude() {
        let body = "void main(){ gl_FragColor = vec4(1.0); }";
        let out = assemble_fragment_source(GlesProfile::V100, body);
        assert!(out.starts_with("#version 100"));
        assert!(out.contains("varying vec2 v_uv;"));
        assert!(out.ends_with(body));
    }

    #[test]
    fn assembly_prepends_v310_prelude() {
        let body = "void main(){ frag_color = vec4(1.0); }";
        let out = assemble_fragment_source(GlesProfile::V310, body);
        assert!(out.starts_with("#version 310 es"));
        assert!(out.contains("in  vec2 v_uv;"));
        assert!(out.contains("out vec4 frag_color;"));
        assert!(out.ends_with(body));
    }

    #[test]
    fn vertex_source_matches_profile() {
        let v100 = vertex_source_for(GlesProfile::V100);
        assert!(v100.starts_with("#version 100"));
        assert!(v100.contains("attribute"));

        let v310 = vertex_source_for(GlesProfile::V310);
        assert!(v310.starts_with("#version 310 es"));
        assert!(v310.contains("in  vec2 a_pos"));
    }

    #[test]
    fn from_meta_maps_versions_to_profiles() {
        assert_eq!(GlesProfile::from_meta(GlesVersion::V100), GlesProfile::V100);
        assert_eq!(GlesProfile::from_meta(GlesVersion::V310), GlesProfile::V310);
    }
}
