//! In-memory shader registry. Walks a directory of paired `.glsl + .toml`
//! files, filters by GLES profile, and exposes lookups.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

use super::meta::{GlesVersion, ShaderMeta};

/// Baked-in panic-fallback shader. Minimal magenta solid so the screen is
/// never black when the active shader fails to compile. Replaced by a richer
/// safe-scene render in sub-plan B (test pattern + version string).
pub const SAFE_SHADER: &str = r#"
void main() {
    gl_FragColor = vec4(1.0, 0.0, 1.0, 1.0);
}
"#;

#[derive(Debug, Clone)]
pub struct LoadedShader {
    pub meta: ShaderMeta,
    pub fragment_body: String,
    pub source_path: PathBuf,
}

#[derive(Debug, Default, Clone)]
pub struct ShaderLibrary {
    shaders: BTreeMap<String, LoadedShader>,
    filtered_count: usize,
}

impl ShaderLibrary {
    /// Load all paired `*.glsl + *.toml` files in `dir`, with no profile filter.
    /// Equivalent to running on the most-capable target (desktop dev / pi5).
    pub fn load_dir(dir: &Path) -> Result<Self> {
        Self::load_dir_for_profile(dir, GlesVersion::V310)
    }

    /// Load all paired files in `dir`, dropping shaders whose `min_gles`
    /// requires a profile newer than `available`. V310 supports any
    /// `min_gles`; V100 supports only V100-tagged shaders.
    pub fn load_dir_for_profile(dir: &Path, available: GlesVersion) -> Result<Self> {
        let mut lib = ShaderLibrary::default();
        lib.inject_safe_shader();
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("glsl") {
                continue;
            }
            // Skip internal preludes / vertex shaders (filenames starting with `_`).
            let stem = match path.file_stem().and_then(|s| s.to_str()) {
                Some(s) if !s.starts_with('_') && !s.ends_with(".vert") => s.to_string(),
                _ => continue,
            };
            let meta_path = path.with_extension("toml");
            if !meta_path.exists() {
                tracing::warn!("shader {} has no .toml metadata, skipping", path.display());
                continue;
            }
            let body = std::fs::read_to_string(&path)?;
            let meta_str = std::fs::read_to_string(&meta_path)?;
            let meta = ShaderMeta::parse(&meta_str, &meta_path.display().to_string())?;
            meta.validate()?;
            if !profile_supports(available, meta.min_gles) {
                tracing::info!(
                    "shader {} requires {:?} (have {:?}); filtered",
                    stem,
                    meta.min_gles,
                    available
                );
                lib.filtered_count += 1;
                continue;
            }
            lib.shaders.insert(
                stem,
                LoadedShader {
                    meta,
                    fragment_body: body,
                    source_path: path,
                },
            );
        }
        Ok(lib)
    }

    pub fn filtered_count(&self) -> usize {
        self.filtered_count
    }

    pub fn get(&self, name: &str) -> Option<&LoadedShader> {
        self.shaders.get(name)
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.shaders.keys().map(|s| s.as_str())
    }

    pub fn require(&self, name: &str) -> Result<&LoadedShader> {
        self.get(name)
            .ok_or_else(|| Error::ShaderNotFound(name.to_string()))
    }

    /// Replace one shader's body+meta — used by hot-reload (sub-plan B).
    pub fn upsert(&mut self, name: &str, shader: LoadedShader) {
        self.shaders.insert(name.to_string(), shader);
    }

    fn inject_safe_shader(&mut self) {
        let meta = ShaderMeta::parse(
            "name = \"__safe__\"\ndisplay_name = \"Safe Fallback\"\n",
            "<baked>",
        )
        .expect("baked safe-shader meta must parse");
        self.shaders.insert(
            "__safe__".to_string(),
            LoadedShader {
                meta,
                fragment_body: SAFE_SHADER.to_string(),
                source_path: PathBuf::from("<baked>"),
            },
        );
    }
}

/// True iff a shader requiring `requires` can run on a binary whose profile
/// supports `available`. V310 supports both V100 and V310. V100 supports
/// only V100.
fn profile_supports(available: GlesVersion, requires: GlesVersion) -> bool {
    matches!(
        (available, requires),
        (GlesVersion::V310, _) | (GlesVersion::V100, GlesVersion::V100)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_paired(dir: &Path, name: &str, glsl: &str, toml: &str) {
        std::fs::write(dir.join(format!("{name}.glsl")), glsl).unwrap();
        std::fs::write(dir.join(format!("{name}.toml")), toml).unwrap();
    }

    #[test]
    fn load_dir_picks_up_paired_files() {
        let tmp = tempfile::tempdir().unwrap();
        write_paired(
            tmp.path(),
            "test",
            include_str!("../../tests/fixtures/shader_good.glsl"),
            include_str!("../../tests/fixtures/shader_good.toml"),
        );
        let lib = ShaderLibrary::load_dir(tmp.path()).unwrap();
        let s = lib.require("test").unwrap();
        assert_eq!(s.meta.params.len(), 2);
        assert!(s.fragment_body.contains("gl_FragColor"));
    }

    #[test]
    fn unpaired_glsl_is_skipped() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("orphan.glsl"), "void main(){}").unwrap();
        let lib = ShaderLibrary::load_dir(tmp.path()).unwrap();
        assert!(lib.get("orphan").is_none());
    }

    #[test]
    fn missing_shader_errors() {
        let lib = ShaderLibrary::default();
        let err = lib.require("nope").unwrap_err();
        assert!(matches!(err, Error::ShaderNotFound(_)));
    }

    #[test]
    fn min_gles_filters_on_v100_target() {
        let tmp = tempfile::tempdir().unwrap();
        write_paired(
            tmp.path(),
            "future",
            "void main(){gl_FragColor=vec4(1);}",
            "name = \"future\"\nmin_gles = \"3.10\"\n",
        );
        let lib_v100 = ShaderLibrary::load_dir_for_profile(tmp.path(), GlesVersion::V100).unwrap();
        assert!(lib_v100.get("future").is_none());
        assert_eq!(lib_v100.filtered_count(), 1);

        let lib_v310 = ShaderLibrary::load_dir_for_profile(tmp.path(), GlesVersion::V310).unwrap();
        assert!(lib_v310.get("future").is_some());
        assert_eq!(lib_v310.filtered_count(), 0);
    }

    #[test]
    fn underscore_files_are_skipped() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("_prelude_100.glsl"), "// prelude").unwrap();
        std::fs::write(
            tmp.path().join("_prelude_100.toml"),
            "name = \"_prelude_100\"\n",
        )
        .unwrap();
        let lib = ShaderLibrary::load_dir(tmp.path()).unwrap();
        assert!(lib.get("_prelude_100").is_none());
    }

    #[test]
    fn safe_shader_always_present() {
        let tmp = tempfile::tempdir().unwrap();
        let lib = ShaderLibrary::load_dir(tmp.path()).unwrap();
        assert!(lib.get("__safe__").is_some());
    }

    #[test]
    fn picks_up_real_passthrough_shader() {
        use std::path::PathBuf;
        let repo_shaders = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("shaders");
        let lib = ShaderLibrary::load_dir(&repo_shaders).unwrap();
        assert!(
            lib.get("passthrough").is_some(),
            "passthrough should load from real shaders/ dir"
        );
        assert!(
            lib.get("__safe__").is_some(),
            "baked safe-shader still present"
        );
    }

    #[test]
    fn all_starter_shaders_load_under_v100() {
        use std::path::PathBuf;
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("shaders");
        let lib = ShaderLibrary::load_dir_for_profile(&dir, GlesVersion::V100).unwrap();
        for name in [
            "passthrough",
            "color_shift",
            "pixelate",
            "kaleidoscope",
            "rgb_glitch",
        ] {
            assert!(lib.get(name).is_some(), "starter shader {name} missing");
        }
    }

    #[test]
    fn starter_shaders_have_no_v310_only_filtered() {
        use std::path::PathBuf;
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("shaders");
        let lib = ShaderLibrary::load_dir_for_profile(&dir, GlesVersion::V100).unwrap();
        assert_eq!(
            lib.filtered_count(),
            0,
            "no starter shader should be v310-only"
        );
    }
}
