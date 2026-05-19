//! Verifies `--gles-profile pi3` (V100) filters V310-only shaders.

use recur::shader::{GlesVersion, ShaderLibrary};
use std::fs;

#[test]
fn v310_only_shader_filtered_under_v100() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("v310_only.glsl"),
        include_str!("fixtures/shader_v310_only.glsl"),
    )
    .unwrap();
    fs::write(
        tmp.path().join("v310_only.toml"),
        include_str!("fixtures/shader_v310_only.toml"),
    )
    .unwrap();

    let lib_v100 = ShaderLibrary::load_dir_for_profile(tmp.path(), GlesVersion::V100).unwrap();
    assert!(lib_v100.get("v310_only").is_none());
    assert_eq!(lib_v100.filtered_count(), 1);

    let lib_v310 = ShaderLibrary::load_dir_for_profile(tmp.path(), GlesVersion::V310).unwrap();
    assert!(lib_v310.get("v310_only").is_some());
}
