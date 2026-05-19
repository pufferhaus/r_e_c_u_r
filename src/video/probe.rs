//! Async codec probe. A background worker thread runs `gstreamer-pbutils`
//! Discoverer on FILES browser focus and pushes results into a shared cache
//! keyed by (canonical path, mtime). FILES browser reads from the cache during
//! render. Unsupported codecs are dimmed and `[X]`-marked; map-to-slot refuses.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::thread::{self, JoinHandle};

use crossbeam_channel::{Receiver, Sender};
use gstreamer as gst;
use gstreamer_pbutils as gst_pbutils;
use gstreamer_pbutils::prelude::DiscovererStreamInfoExt;
use tracing::warn;

use crate::render::shader_assembly::GlesProfile;

/// Single-file probe result. `Supported` carries the short codec name
/// (`h264`, `hevc`, `vp9`, `av1`, etc.); `Unsupported` carries the same
/// short name for status-line messaging.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodecStatus {
    /// Probe enqueued, awaiting worker.
    Pending,
    /// Probe complete, codec runs on the current profile.
    Supported(String),
    /// Probe complete, codec is on the current profile's unsupported list.
    Unsupported(String),
    /// Probe timed out or failed; treated as supported (don't false-block).
    Unknown,
}

/// One probe request enqueued from the browser → worker thread.
#[derive(Debug, Clone)]
pub struct ProbeRequest {
    pub path: PathBuf,
    pub mtime: u64,
}

/// Worker → main result message. Carries everything needed to update the cache.
#[derive(Debug, Clone)]
pub struct ProbeResult {
    pub path: PathBuf,
    pub mtime: u64,
    pub status: CodecStatus,
}

/// Mtime-keyed cache. `(path, mtime)` is the cache key; a different mtime
/// returns None from `get_with_mtime` so callers re-enqueue a probe.
#[derive(Debug, Clone, Default)]
pub struct ProbeCache {
    entries: HashMap<PathBuf, (u64, CodecStatus)>,
}

impl ProbeCache {
    /// Lookup without mtime check (used by render, which doesn't want to
    /// stat the file every frame).
    pub fn get(&self, path: &Path) -> Option<CodecStatus> {
        self.entries.get(path).map(|(_, s)| s.clone())
    }

    /// Lookup with mtime check — returns None if the stored mtime differs,
    /// forcing the caller to re-probe.
    pub fn get_with_mtime(&self, path: &Path, mtime: u64) -> Option<CodecStatus> {
        self.entries
            .get(path)
            .filter(|(m, _)| *m == mtime)
            .map(|(_, s)| s.clone())
    }

    pub fn mark_pending(&mut self, path: &Path, mtime: u64) {
        self.entries
            .insert(path.to_path_buf(), (mtime, CodecStatus::Pending));
    }

    pub fn insert(&mut self, path: &Path, mtime: u64, status: CodecStatus) {
        self.entries.insert(path.to_path_buf(), (mtime, status));
    }

    /// Insert only if the stored entry's mtime matches `mtime` or the cache
    /// has no entry for this path yet. Discards a stale probe result that
    /// arrived after the file was rewritten (newer probe already in flight).
    /// Returns true if the insert happened.
    pub fn insert_if_current(&mut self, path: &Path, mtime: u64, status: CodecStatus) -> bool {
        match self.entries.get(path) {
            Some((m, _)) if *m != mtime => false, // newer entry — discard
            _ => {
                self.entries.insert(path.to_path_buf(), (mtime, status));
                true
            }
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Unsupported codecs for a given runtime profile. V100 (pi3 parity) blocks
/// HEVC/VP9/AV1 which the Pi 3 B+ cannot hardware-decode. V310 (pi5+) accepts
/// all formats supported by `v4l2*` decoders.
pub fn unsupported_for_profile(profile: GlesProfile) -> &'static [&'static str] {
    match profile {
        GlesProfile::V100 => &["hevc", "vp9", "av1"],
        GlesProfile::V310 => &[],
    }
}

/// Map a GStreamer caps string like `video/x-h264` to the short name
/// (`h264`) used in `CodecStatus` and `unsupported_for_profile`.
pub fn short_codec_name(caps_name: &str) -> String {
    let stripped = caps_name.strip_prefix("video/x-").unwrap_or(caps_name);
    match stripped {
        "h265" => "hevc".to_string(),
        other => other.to_string(),
    }
}

/// Apply the per-profile unsupported list to a worker-emitted status.
/// Idempotent for `Pending` / `Unknown` / already-Unsupported.
pub fn reclassify_for_profile(status: CodecStatus, profile: GlesProfile) -> CodecStatus {
    let CodecStatus::Supported(name) = &status else {
        return status;
    };
    if unsupported_for_profile(profile).contains(&name.as_str()) {
        CodecStatus::Unsupported(name.clone())
    } else {
        status
    }
}

/// Handle to a background probe thread. Drop the request-channel `Sender`
/// to signal shutdown; then call `join()`.
pub struct ProbeWorker {
    handle: JoinHandle<()>,
}

impl ProbeWorker {
    /// Spawn a worker that pulls `ProbeRequest`s from `rx`, runs the
    /// gstreamer-pbutils Discoverer (2s timeout), and pushes `ProbeResult`s
    /// to `res_tx`. The thread exits when `rx` is dropped/closed.
    ///
    /// Caller is responsible for ensuring `gst::init()` has been called
    /// before spawning.
    pub fn spawn(rx: Receiver<ProbeRequest>, res_tx: Sender<ProbeResult>) -> Self {
        let handle = thread::Builder::new()
            .name("recur-probe".into())
            .spawn(move || worker_loop(rx, res_tx))
            .expect("probe worker spawn");
        Self { handle }
    }

    pub fn join(self) -> std::thread::Result<()> {
        self.handle.join()
    }
}

fn worker_loop(rx: Receiver<ProbeRequest>, res_tx: Sender<ProbeResult>) {
    let timeout = gst::ClockTime::from_seconds(2);
    let disc = match gst_pbutils::Discoverer::new(timeout) {
        Ok(d) => d,
        Err(e) => {
            warn!("probe worker init failed: {e}; thread exits");
            return;
        }
    };
    while let Ok(req) = rx.recv() {
        let status = probe_one(&disc, &req.path);
        let result = ProbeResult {
            path: req.path,
            mtime: req.mtime,
            status,
        };
        if res_tx.send(result).is_err() {
            return;
        }
    }
}

fn probe_one(disc: &gst_pbutils::Discoverer, path: &Path) -> CodecStatus {
    let uri = match url_from_path(path) {
        Some(u) => u,
        None => return CodecStatus::Unknown,
    };
    let info = match disc.discover_uri(&uri) {
        Ok(i) => i,
        Err(e) => {
            warn!("probe {}: {e}", path.display());
            return CodecStatus::Unknown;
        }
    };
    let video_streams = info.video_streams();
    let Some(first) = video_streams.first() else {
        return CodecStatus::Unknown;
    };
    let Some(caps) = first.caps() else {
        return CodecStatus::Unknown;
    };
    let Some(structure) = caps.structure(0) else {
        return CodecStatus::Unknown;
    };
    let codec = short_codec_name(structure.name().as_str());
    CodecStatus::Supported(codec)
}

fn url_from_path(p: &Path) -> Option<String> {
    if p.is_absolute() {
        Some(format!("file://{}", p.display()))
    } else {
        let canon = std::fs::canonicalize(p).ok()?;
        Some(format!("file://{}", canon.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn insert_if_current_discards_stale_mtime() {
        let mut cache = ProbeCache::default();
        let p = PathBuf::from("/file.mp4");
        cache.insert(&p, 200, CodecStatus::Pending);
        // Stale probe (mtime 100) lands after the cache was updated to mtime 200.
        let inserted = cache.insert_if_current(&p, 100, CodecStatus::Supported("h264".into()));
        assert!(!inserted, "stale insert must be rejected");
        assert_eq!(cache.get(&p), Some(CodecStatus::Pending));
    }

    #[test]
    fn insert_if_current_accepts_matching_mtime() {
        let mut cache = ProbeCache::default();
        let p = PathBuf::from("/file.mp4");
        cache.insert(&p, 100, CodecStatus::Pending);
        let inserted = cache.insert_if_current(&p, 100, CodecStatus::Supported("h264".into()));
        assert!(inserted);
        assert_eq!(cache.get(&p), Some(CodecStatus::Supported("h264".into())));
    }

    #[test]
    fn insert_if_current_accepts_first_entry() {
        let mut cache = ProbeCache::default();
        let p = PathBuf::from("/file.mp4");
        let inserted = cache.insert_if_current(&p, 100, CodecStatus::Supported("h264".into()));
        assert!(inserted);
    }

    #[test]
    fn unsupported_lists_match_target_table() {
        use crate::render::shader_assembly::GlesProfile;
        let v100 = unsupported_for_profile(GlesProfile::V100);
        assert!(v100.contains(&"hevc"));
        assert!(v100.contains(&"vp9"));
        assert!(v100.contains(&"av1"));
        let v310 = unsupported_for_profile(GlesProfile::V310);
        assert!(v310.is_empty());
    }

    #[test]
    fn cache_returns_none_for_unknown_path() {
        let cache = ProbeCache::default();
        assert_eq!(cache.get(&PathBuf::from("/nope")), None);
    }

    #[test]
    fn cache_round_trips_pending_then_resolved() {
        let mut cache = ProbeCache::default();
        let p = PathBuf::from("/some/file.mp4");
        cache.mark_pending(&p, 12345);
        assert_eq!(cache.get(&p), Some(CodecStatus::Pending));
        cache.insert(&p, 12345, CodecStatus::Supported("h264".into()));
        assert_eq!(cache.get(&p), Some(CodecStatus::Supported("h264".into())));
    }

    #[test]
    fn cache_mtime_change_invalidates_entry() {
        let mut cache = ProbeCache::default();
        let p = PathBuf::from("/file.mp4");
        cache.insert(&p, 100, CodecStatus::Supported("h264".into()));
        assert_eq!(cache.get_with_mtime(&p, 200), None);
        assert_eq!(
            cache.get_with_mtime(&p, 100),
            Some(CodecStatus::Supported("h264".into()))
        );
    }

    #[test]
    fn codec_status_unsupported_carries_name() {
        let s = CodecStatus::Unsupported("hevc".into());
        match s {
            CodecStatus::Unsupported(n) => assert_eq!(n, "hevc"),
            _ => panic!("expected Unsupported variant"),
        }
    }

    #[test]
    fn probe_worker_starts_and_stops_cleanly() {
        gstreamer::init().ok();
        let (tx, rx) = crossbeam_channel::unbounded();
        let (res_tx, _res_rx) = crossbeam_channel::unbounded::<ProbeResult>();
        let worker = ProbeWorker::spawn(rx, res_tx);
        drop(tx); // close request channel → worker exits
        worker.join().expect("worker thread must join cleanly");
    }

    #[test]
    fn reclassify_marks_hevc_unsupported_on_v100() {
        use crate::render::shader_assembly::GlesProfile;
        let s = CodecStatus::Supported("hevc".into());
        let out = reclassify_for_profile(s, GlesProfile::V100);
        assert_eq!(out, CodecStatus::Unsupported("hevc".into()));
    }

    #[test]
    fn reclassify_passes_h264_through_on_v100() {
        use crate::render::shader_assembly::GlesProfile;
        let s = CodecStatus::Supported("h264".into());
        let out = reclassify_for_profile(s, GlesProfile::V100);
        assert_eq!(out, CodecStatus::Supported("h264".into()));
    }

    #[test]
    fn reclassify_passes_everything_on_v310() {
        use crate::render::shader_assembly::GlesProfile;
        for codec in ["h264", "hevc", "vp9", "av1"] {
            let s = CodecStatus::Supported(codec.into());
            assert_eq!(
                reclassify_for_profile(s, GlesProfile::V310),
                CodecStatus::Supported(codec.into())
            );
        }
    }

    #[test]
    fn reclassify_leaves_unknown_alone() {
        use crate::render::shader_assembly::GlesProfile;
        assert_eq!(
            reclassify_for_profile(CodecStatus::Unknown, GlesProfile::V100),
            CodecStatus::Unknown
        );
        assert_eq!(
            reclassify_for_profile(CodecStatus::Pending, GlesProfile::V100),
            CodecStatus::Pending
        );
    }
}
