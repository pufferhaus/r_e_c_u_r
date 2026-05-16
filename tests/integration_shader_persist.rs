//! Smoke test: ensure shader_banks.toml round-trips across runs (without
//! actually starting the render loop).

use recur::persist;
use recur::shader::{ShaderBank, ShaderSlot};

#[test]
fn shader_banks_persist_through_save_load_cycle() {
    let tmp = tempfile::tempdir().unwrap();
    let mut b = ShaderBank::empty();
    b.slots[1] = Some(ShaderSlot {
        shader: "color_shift".into(),
        params: [0.3, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    });
    persist::save_shader_banks(tmp.path(), &[b.clone()]).unwrap();
    let got = persist::load_shader_banks(tmp.path()).unwrap();
    assert_eq!(got, vec![b]);
}
