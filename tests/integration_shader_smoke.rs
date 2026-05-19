//! Compile each starter shader against both GLES preludes (pure-Rust assembly
//! path, no GL context required) — verifies they at least produce textually-
//! valid source strings.

use recur::render::shader_assembly::{assemble_fragment_source, GlesProfile};

fn shader_body(name: &str) -> String {
    let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("shaders")
        .join(format!("{name}.glsl"));
    std::fs::read_to_string(&p).unwrap_or_else(|_| panic!("read {p:?}"))
}

#[test]
fn all_starters_assemble_under_v100() {
    for name in [
        "passthrough",
        "color_shift",
        "pixelate",
        "kaleidoscope",
        "rgb_glitch",
    ] {
        let body = shader_body(name);
        let src = assemble_fragment_source(GlesProfile::V100, &body);
        assert!(
            src.starts_with("#version 100"),
            "{name}: V100 prelude missing"
        );
        assert!(
            src.contains("gl_FragColor"),
            "{name}: shader must write gl_FragColor in V100"
        );
    }
}
