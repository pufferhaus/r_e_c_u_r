//! Smoke test: ShaderWatcher start + drain sequence does not panic and emits
//! at least one event for a touched file.

use recur::shader::ShaderWatcher;
use std::time::Duration;

#[test]
fn watcher_starts_and_drains() {
    let tmp = tempfile::tempdir().unwrap();
    let w = ShaderWatcher::start(tmp.path()).unwrap();
    std::fs::write(tmp.path().join("color_shift.glsl"), b"void main(){}").unwrap();
    std::thread::sleep(Duration::from_millis(500));
    let events = w.try_drain();
    assert!(
        !events.is_empty(),
        "watcher should emit ≥1 event for fs write"
    );
}
