//! `notify`-backed file watcher that emits `ShaderEvent::Dirty(name)` on
//! `.glsl` or `.toml` saves in the shaders dir. The watcher thread is
//! lightweight; the render loop drains the channel between frames and
//! invokes `ShaderPipeline::reload(name)` (see Task 12).
//!
//! Compile failures are *not* the watcher's concern — it just signals dirt.

use std::path::Path;

use crossbeam_channel::{unbounded, Receiver};
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShaderEvent {
    /// Some `.glsl` or `.toml` file in the watched dir was modified. The
    /// payload is the file stem (e.g. "color_shift").
    Dirty(String),
}

pub struct ShaderWatcher {
    rx: Receiver<ShaderEvent>,
    _watcher: RecommendedWatcher,
}

impl ShaderWatcher {
    pub fn start(dir: &Path) -> notify::Result<Self> {
        let (tx, rx) = unbounded();
        let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if let Ok(ev) = res {
                if matches!(ev.kind, EventKind::Modify(_) | EventKind::Create(_)) {
                    for p in ev.paths {
                        if let Some(name) = shader_name_from_path(&p) {
                            let _ = tx.send(ShaderEvent::Dirty(name));
                        }
                    }
                }
            }
        })?;
        watcher.watch(dir, RecursiveMode::NonRecursive)?;
        Ok(Self { rx, _watcher: watcher })
    }

    pub fn try_drain(&self) -> Vec<ShaderEvent> {
        self.rx.try_iter().collect()
    }
}

fn shader_name_from_path(p: &Path) -> Option<String> {
    let stem = p.file_stem()?.to_str()?;
    if stem.starts_with('_') {
        return None;
    }
    let ext = p.extension()?.to_str()?;
    if ext != "glsl" && ext != "toml" {
        return None;
    }
    Some(stem.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn shader_name_from_path_picks_stems() {
        assert_eq!(shader_name_from_path(Path::new("/a/foo.glsl")).as_deref(), Some("foo"));
        assert_eq!(shader_name_from_path(Path::new("/a/foo.toml")).as_deref(), Some("foo"));
        assert_eq!(shader_name_from_path(Path::new("/a/_prelude.glsl")).as_deref(), None);
        assert_eq!(shader_name_from_path(Path::new("/a/foo.vert")).as_deref(), None);
        assert_eq!(shader_name_from_path(Path::new("/a/foo.txt")).as_deref(), None);
    }

    #[test]
    fn watcher_emits_dirty_on_file_write() {
        let tmp = tempfile::tempdir().unwrap();
        let w = ShaderWatcher::start(tmp.path()).unwrap();
        std::fs::write(tmp.path().join("color_shift.glsl"), b"void main(){}").unwrap();
        // notify is async — give it a moment.
        std::thread::sleep(Duration::from_millis(500));
        let events = w.try_drain();
        assert!(
            events.iter().any(|e| matches!(e, ShaderEvent::Dirty(n) if n == "color_shift")),
            "expected Dirty(\"color_shift\"), got {events:?}"
        );
    }
}
