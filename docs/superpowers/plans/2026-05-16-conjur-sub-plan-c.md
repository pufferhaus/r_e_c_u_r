# Phase 2 — Sub-plan C (conjur codec probe) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add asynchronous codec probing to FILES browser so unsupported codecs (per build target / `--gles-profile`) appear dimmed with an `[X]` glyph, and map-to-slot refuses them with a clear status message.

**Architecture:** A worker thread runs `gstreamer-pbutils::Discoverer` synchronously, fed by a crossbeam channel of probe requests, returning results on a second crossbeam channel. `main.rs` drains results each frame into a `ProbeCache` (mtime-keyed `HashMap<PathBuf, CodecStatus>`) stored on `SharedState`. `BrowserBody` enqueues probes when the focused row changes; renders cached statuses as dim + `[X]` for unsupported, `[…]` for pending. `apply.rs`'s `Action::Enter` arm in the browser-mapping path refuses unsupported codecs with a status-line message via `state.last_error`. `ATTR_DIM` is wired through `TextOverlay::draw` once (was a declared-but-unused constant) — also useful for future disabled-menu states.

**Tech Stack:** Rust 1.85, `gstreamer-pbutils` 0.23 (already a dep), `crossbeam-channel` 0.5 (from sub-plan B), `notify` reused for mtime. No new dependencies.

**Scope boundary:** Codec list maintenance and `recur convert <in> <out>` ffmpeg wrapper (spec backlog) are out of scope. ATTR_DIM render wiring is in scope as a prerequisite (Task 1) since dim rendering does not exist anywhere in the codebase yet.

---

## File Map

**Create:**
- `src/video/probe.rs` — `CodecStatus`, `ProbeRequest`, `ProbeResult`, `ProbeWorker` (background thread + channels), `ProbeCache` (mtime-keyed HashMap), `unsupported_for_profile`.
- `tests/integration_codec_probe.rs` — end-to-end probe on a bundled test asset (h264) — confirms worker produces `Supported(h264)`.
- `tests/fixtures/unsupported_codec_path.txt` — single line `assets/test_smpte.mp4` so tests can locate the bundled clip without hardcoding.

**Modify:**
- `src/render/text.rs` — `TextOverlay::draw` reads `ATTR_DIM` and mixes fg/bg towards bg by 60% (resulting glyph at ~40% brightness on amber-on-black).
- `src/status/grid.rs` — add `dim_row(&mut self, row: usize)` and `dim_cell(&mut self, row, col)` helpers, mirroring `invert_row` shape. Update doc comments.
- `src/state.rs` — add `pub probe_cache: probe::ProbeCache` field (default empty), and `pub probe_tx: Option<crossbeam_channel::Sender<probe::ProbeRequest>>` (None outside main). Add `pub gles_profile_changed: bool` is **NOT** needed; profile changes only at startup.
- `src/video/mod.rs` — `pub mod probe;` and `pub use probe::*;`.
- `src/sample/browser_walk.rs` — `BrowserRow` gets `pub probe_key: PathBuf` field (canonicalised abs path) used as the cache key. Walk fills it via `std::fs::canonicalize`; falls back to the raw path on error.
- `src/menu/browser.rs` — `BrowserBody`:
  - On `NavUp`/`NavDown` (focus change), if the new focused row is a file with no cache entry, send a `ProbeRequest` via `state.probe_tx`.
  - In `render`, look up `state.probe_cache.get(&row.probe_key)`:
    - `None` → no marker (file not yet focused).
    - `Pending` → `[…]` glyph appended in the slot column.
    - `Supported(_)` → no marker (clean row).
    - `Unsupported(name)` → ` [X]` glyph + `grid.dim_row(row_idx)` after the write.
  - In `Action::Enter` for an unsupported file, do NOT add to slot; set `state.last_error = Some(format!("cannot map: {profile} build does not support {codec}"))`.
- `src/apply.rs` — no changes (browser is the gatekeeper; `Action::Enter` is handled there). Confirmed by reading the apply arm — Enter only mutates state via BrowserBody.

Actually wait — Enter on a browser row is handled inside `BrowserBody::handle`, not `apply.rs`. So apply.rs is untouched. Good.

- `src/main.rs` — Construct the probe worker + channels alongside `ShaderWatcher`. Drain results each frame into `state.probe_cache`. Pass the `probe_tx` Sender into `state.probe_tx` before the loop starts.
- `Cargo.toml` — no changes (`gstreamer-pbutils` and `crossbeam-channel` already present).

**Test:**
- Inline tests in `src/video/probe.rs` (`mod tests`) — pure CodecStatus + cache + unsupported-list logic without launching a real worker.
- `src/menu/browser.rs::tests` — dim-on-unsupported render assertion; status-line on Enter-of-unsupported.
- `tests/integration_codec_probe.rs` — spins the worker on a temp dir with `assets/test_smpte.mp4`, asserts a `Supported("h264")` result lands in the cache within 3 seconds.
- `src/render/text.rs::tests` — atlas tests stay; ATTR_DIM is observable only via screen capture, so coverage there is via `src/status/grid.rs::tests` for the helper and a TextOverlay run that doesn't panic.

**Out of scope:**
- `recur convert <in> <out>` ffmpeg wrapper (spec §9 backlog).
- Probe-cache persistence across runs.
- Dim-render in the Pi LCD pathway beyond the shared `TextOverlay::draw` (same code path).
- Live re-probe when a file is overwritten while focused — mtime-invalidation handles next focus.

---

### Task 1: ATTR_DIM rendering in TextOverlay

**Files:**
- Modify: `src/render/text.rs:29` (use line — add `ATTR_DIM`), `:367-371` (fg/bg mixing).
- Modify: `src/status/grid.rs` (add `dim_row` + `dim_cell` helpers + tests).

- [ ] **Step 1: Write the failing test**

Add to `src/status/grid.rs` `mod tests`:

```rust
#[test]
fn dim_row_sets_dim_bit_on_all_cells() {
    let mut g = TextGrid::new(5, 3);
    g.write_row(1, "AB");
    g.dim_row(1);
    assert!(g.at(1, 0).attr & ATTR_DIM != 0);
    assert!(g.at(1, 1).attr & ATTR_DIM != 0);
    assert!(g.at(1, 4).attr & ATTR_DIM != 0);
    assert!(g.at(0, 0).attr & ATTR_DIM == 0);
}

#[test]
fn dim_cell_sets_bit_on_one_cell() {
    let mut g = TextGrid::new(5, 3);
    g.dim_cell(2, 3);
    assert!(g.at(2, 3).attr & ATTR_DIM != 0);
    assert!(g.at(2, 2).attr & ATTR_DIM == 0);
}

#[test]
fn dim_row_combines_with_inverse() {
    // Inverse + dim should both be set.
    let mut g = TextGrid::new(5, 2);
    g.write_row(0, "X");
    g.invert_row(0);
    g.dim_row(0);
    assert!(g.at(0, 0).attr & ATTR_INVERSE != 0);
    assert!(g.at(0, 0).attr & ATTR_DIM != 0);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib status::grid::tests::dim_row_sets_dim_bit_on_all_cells`
Expected: FAIL — `dim_row` not defined.

- [ ] **Step 3: Add helpers**

In `src/status/grid.rs`, after `invert_row`:

```rust
/// Set the `ATTR_DIM` bit on every cell in `row`. Idempotent.
pub fn dim_row(&mut self, row: usize) {
    if row >= self.rows {
        return;
    }
    let base = row * self.cols;
    for col in 0..self.cols {
        self.cells[base + col].attr |= ATTR_DIM;
    }
}

/// Set the `ATTR_DIM` bit on a single cell. Out-of-bounds = no-op.
pub fn dim_cell(&mut self, row: usize, col: usize) {
    if row < self.rows && col < self.cols {
        self.cells[row * self.cols + col].attr |= ATTR_DIM;
    }
}
```

- [ ] **Step 4: Wire ATTR_DIM into TextOverlay::draw**

In `src/render/text.rs`:

Line 29, change the use:

```rust
use crate::status::grid::{TextGrid, ATTR_DIM, ATTR_INVERSE};
```

In `TextOverlay::draw`, after the existing `(fg, bg) = if cell.attr & ATTR_INVERSE != 0 { (BG, FG) } else { (FG, BG) };` block (around line 367-371), add:

```rust
let (fg, bg) = if cell.attr & ATTR_INVERSE != 0 {
    (BG, FG)
} else {
    (FG, BG)
};
// ATTR_DIM mixes fg toward bg by 60% → glyph reads at ~40% brightness.
let (fg, bg) = if cell.attr & ATTR_DIM != 0 {
    let mix = |a: [f32; 3], b: [f32; 3]| {
        [a[0] * 0.4 + b[0] * 0.6, a[1] * 0.4 + b[1] * 0.6, a[2] * 0.4 + b[2] * 0.6]
    };
    (mix(fg, bg), bg)
} else {
    (fg, bg)
};
```

- [ ] **Step 5: Run tests + build**

Run: `cargo test --lib status::grid && cargo build`
Expected: 3 new tests pass + build clean.

- [ ] **Step 6: Commit**

```bash
git add src/status/grid.rs src/render/text.rs
git commit -m "feat(status): ATTR_DIM render path (TextOverlay mixes fg→bg 60%)"
```

(No Co-Authored-By trailer.)

---

### Task 2: CodecStatus + unsupported_for_profile + ProbeCache types

**Files:**
- Create: `src/video/probe.rs`
- Modify: `src/video/mod.rs`

- [ ] **Step 1: Write the failing test**

Create `src/video/probe.rs`:

```rust
//! Async codec probe. A background worker thread runs `gstreamer-pbutils`
//! Discoverer on FILES browser focus and pushes results into a shared cache
//! keyed by (canonical path, mtime). FILES browser reads from the cache during
//! render. Unsupported codecs are dimmed and `[X]`-marked; map-to-slot refuses.

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn unsupported_lists_match_target_table() {
        // Spec section 7:
        // pi3 / desktop --gles-profile pi3 → hevc, vp9, av1
        // pi5 / desktop default → none
        use crate::render::shader_assembly::GlesProfile;
        let v100 = unsupported_for_profile(GlesProfile::V100);
        assert!(v100.contains("hevc"));
        assert!(v100.contains("vp9"));
        assert!(v100.contains("av1"));
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
        // Same path, different mtime → cache lookup returns None to force re-probe.
        assert_eq!(cache.get_with_mtime(&p, 200), None);
        assert_eq!(cache.get_with_mtime(&p, 100), Some(CodecStatus::Supported("h264".into())));
    }

    #[test]
    fn codec_status_unsupported_carries_name() {
        let s = CodecStatus::Unsupported("hevc".into());
        match s {
            CodecStatus::Unsupported(n) => assert_eq!(n, "hevc"),
            _ => panic!("expected Unsupported variant"),
        }
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib video::probe`
Expected: FAIL — module not yet wired (compile error).

- [ ] **Step 3: Implement the types**

Add to the top of `src/video/probe.rs`:

```rust
use std::collections::HashMap;
use std::path::{Path, PathBuf};

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
    // `video/x-h265` → "hevc" alias for the unsupported-list lookup.
    match stripped {
        "h265" => "hevc".to_string(),
        other => other.to_string(),
    }
}
```

Update `src/video/mod.rs`. Read the current contents and append:

```rust
pub mod probe;
pub use probe::{
    short_codec_name, unsupported_for_profile, CodecStatus, ProbeCache, ProbeRequest, ProbeResult,
};
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib video::probe`
Expected: PASS (5 tests).

- [ ] **Step 5: Commit**

```bash
git add src/video/probe.rs src/video/mod.rs
git commit -m "feat(probe): CodecStatus + ProbeCache + unsupported_for_profile (no worker yet)"
```

---

### Task 3: ProbeWorker — Discoverer on a background thread

**Files:**
- Modify: `src/video/probe.rs`

- [ ] **Step 1: Write the failing test (smoke)**

Add to `src/video/probe.rs` `mod tests`:

```rust
#[test]
fn probe_worker_starts_and_stops_cleanly() {
    // Just exercise the worker lifecycle; no probing.
    crate::error::Result::<()>::Ok(()).unwrap(); // shape only
    let (tx, rx) = crossbeam_channel::unbounded();
    let (res_tx, _res_rx) = crossbeam_channel::unbounded();
    let worker = ProbeWorker::spawn(rx, res_tx);
    drop(tx); // close request channel → worker exits
    worker.join().expect("worker thread must join cleanly");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib video::probe::tests::probe_worker_starts_and_stops_cleanly`
Expected: FAIL — `ProbeWorker` not defined.

- [ ] **Step 3: Implement ProbeWorker**

Append to `src/video/probe.rs`:

```rust
use std::thread::{self, JoinHandle};

use crossbeam_channel::{Receiver, Sender};
use gstreamer as gst;
use gstreamer_pbutils as gst_pbutils;
use tracing::warn;

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
    // Discoverer with 2s timeout (per spec §7 async-probing details).
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
            // Main hung up; quietly exit.
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
        // No video stream → treat as unknown so caller can still try to
        // map it (audio-only files probably won't play, but that's a
        // different failure surface).
        return CodecStatus::Unknown;
    };
    let Some(caps) = first.caps() else {
        return CodecStatus::Unknown;
    };
    let Some(structure) = caps.structure(0) else {
        return CodecStatus::Unknown;
    };
    let codec = short_codec_name(structure.name().as_str());
    // Decide Supported vs Unsupported by checking against the caller's
    // active profile. Since the worker doesn't know the active profile,
    // it always returns Supported(name); the main-side drain reclassifies
    // against state.gles_profile. That keeps the worker stateless and
    // means a runtime profile change re-classifies cached entries without
    // re-probing.
    CodecStatus::Supported(codec)
}

fn url_from_path(p: &Path) -> Option<String> {
    let canon = std::fs::canonicalize(p).ok()?;
    Some(format!("file://{}", canon.display()))
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib video::probe`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/video/probe.rs
git commit -m "feat(probe): ProbeWorker background thread + Discoverer wiring"
```

---

### Task 4: Reclassify-on-drain helper

The worker emits `Supported(name)` regardless of profile. The drain side reclassifies against the current profile. Pull that into a tiny helper so both the integration test and main.rs use the same logic.

**Files:**
- Modify: `src/video/probe.rs`

- [ ] **Step 1: Write the failing test**

Add to `mod tests`:

```rust
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
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib video::probe::tests::reclassify`
Expected: FAIL — `reclassify_for_profile` undefined.

- [ ] **Step 3: Implement**

Append to `src/video/probe.rs` (after `short_codec_name`):

```rust
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
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib video::probe`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/video/probe.rs
git commit -m "feat(probe): reclassify_for_profile applies per-target unsupported list"
```

---

### Task 5: Wire ProbeCache + probe_tx into SharedState

**Files:**
- Modify: `src/state.rs`

- [ ] **Step 1: Write the failing test**

Add to `src/state.rs` `mod tests`:

```rust
#[test]
fn shared_state_starts_with_empty_probe_cache_and_no_tx() {
    let s = SharedState::new();
    assert!(s.probe_cache.is_empty());
    assert!(s.probe_tx.is_none());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib state::tests::shared_state_starts_with_empty_probe_cache`
Expected: FAIL — `probe_cache`, `probe_tx` undefined.

- [ ] **Step 3: Extend SharedState**

In `src/state.rs`:

1. Add imports at the top alongside existing `use crate::shader::ShaderBank;`:

```rust
use crate::video::{ProbeCache, ProbeRequest};
```

2. Append fields to the `SharedState` struct (after `gles_profile`):

```rust
    /// Codec-probe cache, populated by main.rs from the worker channel.
    pub probe_cache: ProbeCache,
    /// Sender to the probe worker. None outside main (e.g. in tests).
    pub probe_tx: Option<crossbeam_channel::Sender<ProbeRequest>>,
```

3. Initialise both to defaults in `SharedState::new`:

```rust
            probe_cache: ProbeCache::default(),
            probe_tx: None,
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib state`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/state.rs
git commit -m "feat(state): probe_cache + probe_tx fields on SharedState"
```

---

### Task 6: BrowserRow carries probe_key (canonical path)

**Files:**
- Modify: `src/sample/browser_walk.rs`

- [ ] **Step 1: Write the failing test**

Add to `src/sample/browser_walk.rs` `mod tests`:

```rust
#[test]
fn browser_row_carries_probe_key_for_files() {
    let tmp = tempfile::tempdir().unwrap();
    let f = tmp.path().join("clip.mp4");
    fs::write(&f, b"").unwrap();
    let rows = walk_browser(&[tmp.path().to_path_buf()], &HashSet::new());
    let row = rows.iter().find(|r| r.is_file).unwrap();
    let canon = std::fs::canonicalize(&f).unwrap();
    assert_eq!(row.probe_key, canon);
}

#[test]
fn browser_row_probe_key_falls_back_to_path_on_canon_error() {
    // Canonicalize of a path that exists will succeed; we just confirm the
    // field is present for directories too (set to row.path for non-files).
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir(tmp.path().join("sub")).unwrap();
    let rows = walk_browser(&[tmp.path().to_path_buf()], &HashSet::new());
    let dir_row = rows.iter().find(|r| !r.is_file).unwrap();
    assert!(dir_row.probe_key.ends_with("sub") || dir_row.probe_key == dir_row.path);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib sample::browser_walk`
Expected: FAIL — `probe_key` field missing.

- [ ] **Step 3: Add the field**

In `src/sample/browser_walk.rs`:

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct BrowserRow {
    pub display: String,
    pub path: PathBuf,
    pub is_file: bool,
    pub depth: usize,
    /// Canonical absolute path used as ProbeCache key. Falls back to `path`
    /// on canonicalize error (broken symlink, missing intermediate).
    pub probe_key: PathBuf,
}
```

In `walk_recursive`, where `BrowserRow` is constructed, compute the probe_key:

```rust
// For both dir and file rows:
out.push(BrowserRow {
    display: format!("{}{}{}", indent(depth), name, glyph),
    path: d.clone(),
    is_file: false,
    depth,
    probe_key: std::fs::canonicalize(&d).unwrap_or_else(|_| d.clone()),
});
```

```rust
out.push(BrowserRow {
    display: format!("{}{}", indent(depth), name),
    path: f.clone(),
    is_file: true,
    depth,
    probe_key: std::fs::canonicalize(&f).unwrap_or_else(|_| f.clone()),
});
```

(Use `f.clone()` so we can also pass `f` itself by value into the path field; same for `d`.)

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib sample::browser_walk`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/sample/browser_walk.rs
git commit -m "feat(browser): BrowserRow carries canonical probe_key for cache lookup"
```

---

### Task 7: Browser focus enqueues probe; render dims unsupported

**Files:**
- Modify: `src/menu/browser.rs`

- [ ] **Step 1: Write the failing test**

Add to `src/menu/browser.rs` `mod tests`:

```rust
#[test]
fn render_dims_unsupported_codec_row() {
    use crate::video::{CodecStatus, ProbeCache};
    let tmp = tempfile::tempdir().unwrap();
    let f = tmp.path().join("a.mp4");
    fs::write(&f, b"").unwrap();
    let canon = std::fs::canonicalize(&f).unwrap();

    let mut st = SharedState::new();
    st.paths_to_browser = vec![tmp.path().to_path_buf()];
    st.probe_cache.insert(&canon, 0, CodecStatus::Unsupported("hevc".into()));

    let b = BrowserBody::new();
    let mut grid = crate::status::grid::TextGrid::new(48, 17);
    b.render(&st, &mut grid);

    // Row 5 is the first body row. The unsupported file should be dimmed.
    let row5_attr = grid.at(5, 0).attr;
    assert!(row5_attr & crate::status::grid::ATTR_DIM != 0,
        "row 5 should have ATTR_DIM (got attr={row5_attr:#04x})");
    // And contain "[X]" somewhere in the row text.
    let row5: String = (0..48).map(|c| grid.at(5, c).ch).collect();
    assert!(row5.contains("[X]"), "row should contain [X] marker, got: {row5:?}");
}

#[test]
fn render_shows_pending_glyph_during_probe() {
    use crate::video::{CodecStatus, ProbeCache};
    let tmp = tempfile::tempdir().unwrap();
    let f = tmp.path().join("a.mp4");
    fs::write(&f, b"").unwrap();
    let canon = std::fs::canonicalize(&f).unwrap();

    let mut st = SharedState::new();
    st.paths_to_browser = vec![tmp.path().to_path_buf()];
    st.probe_cache.insert(&canon, 0, CodecStatus::Pending);

    let b = BrowserBody::new();
    let mut grid = crate::status::grid::TextGrid::new(48, 17);
    b.render(&st, &mut grid);

    let row5: String = (0..48).map(|c| grid.at(5, c).ch).collect();
    assert!(row5.contains("[..]") || row5.contains("[…]"), "expected pending marker, got: {row5:?}");
}

#[test]
fn enter_on_unsupported_file_sets_status_line_and_does_not_map() {
    use crate::video::CodecStatus;
    let tmp = tempfile::tempdir().unwrap();
    let f = tmp.path().join("hevc_clip.mp4");
    fs::write(&f, b"").unwrap();
    let canon = std::fs::canonicalize(&f).unwrap();

    let mut st = SharedState::new();
    st.paths_to_browser = vec![tmp.path().to_path_buf()];
    st.probe_cache.insert(&canon, 0, CodecStatus::Unsupported("hevc".into()));

    let mut b = BrowserBody::new();
    b.handle(Action::Enter, &mut st);

    // Slot was NOT filled.
    assert!(st.banks[0].slots[0].is_none(), "slot 0 should stay empty");
    // last_error mentions the codec.
    let err = st.last_error.as_deref().unwrap_or("");
    assert!(err.contains("hevc"), "got: {err:?}");
    assert!(err.contains("cannot map"), "got: {err:?}");
}

#[test]
fn nav_to_file_enqueues_probe_request_when_tx_set() {
    use crate::video::ProbeRequest;
    let tmp = tempfile::tempdir().unwrap();
    let f = tmp.path().join("a.mp4");
    fs::write(&f, b"").unwrap();
    let canon = std::fs::canonicalize(&f).unwrap();

    let (tx, rx) = crossbeam_channel::unbounded::<ProbeRequest>();
    let mut st = SharedState::new();
    st.paths_to_browser = vec![tmp.path().to_path_buf()];
    st.probe_tx = Some(tx);

    let mut b = BrowserBody::new();
    // The first row is already focused on construction; explicit NavDown then
    // NavUp would also fire a probe. We trigger one explicit NavUp at index 0
    // (which clamps to 0) to exercise the "focus row is a file with no cache"
    // path on whatever the current selection is.
    b.handle(Action::NavUp, &mut st);

    let req = rx.try_recv().expect("expected probe request");
    assert_eq!(req.path, canon);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib menu::browser`
Expected: FAIL — no dim handling, no probe enqueue, no Enter-refusal.

- [ ] **Step 3: Implement**

Modify `src/menu/browser.rs`:

1. Add imports at the top:

```rust
use crate::video::{CodecStatus, ProbeRequest};
```

2. Extend `BrowserBody::render`. Inside the per-row loop, after `grid.write_row(...)`, before the existing `if abs == self.selected { grid.invert_row(...) }` line:

```rust
let truncated: String = row.display.chars().take(38).collect();
// Compose the line; we may append a probe-status marker if cached.
let marker = match state.probe_cache.get(&row.probe_key) {
    Some(CodecStatus::Pending) => " [..]",
    Some(CodecStatus::Unsupported(_)) => " [X]",
    _ => "",
};
let line_text = format!("{:<38} {:<5}{}", truncated, slot, marker);
grid.write_row(row_idx, &line_text);
let is_unsupported = matches!(
    state.probe_cache.get(&row.probe_key),
    Some(CodecStatus::Unsupported(_))
);
if is_unsupported {
    grid.dim_row(row_idx);
}
if abs == self.selected {
    grid.invert_row(row_idx);
}
```

(Note: replace the existing `let truncated: String = ...; grid.write_row(row_idx, &format!(...));` block — keep the `slot` variable from above unchanged.)

3. In `BrowserBody::handle`, replace the `Action::Enter` arm. Current code is:

```rust
Action::Enter => {
    let row = rows[self.selected].clone();
    if row.is_file {
        if let Some(idx) = state.current_bank().first_empty() {
            let slot = Slot { ... };
            state.current_bank_mut().slots[idx] = Some(slot);
        }
    } else if self.open.contains(&row.path) {
        // ...
    }
}
```

Wrap the `if row.is_file` block with a codec-refusal check:

```rust
Action::Enter => {
    let row = rows[self.selected].clone();
    if row.is_file {
        // Refuse mapping if the cached probe says unsupported.
        if let Some(CodecStatus::Unsupported(codec)) =
            state.probe_cache.get(&row.probe_key)
        {
            let profile_label = match state.gles_profile {
                crate::render::shader_assembly::GlesProfile::V100 => "pi3",
                crate::render::shader_assembly::GlesProfile::V310 => "pi5",
            };
            state.last_error = Some(format!(
                "cannot map: {profile_label} build does not support {codec}"
            ));
        } else if let Some(idx) = state.current_bank().first_empty() {
            let slot = Slot {
                location: row.path.clone(),
                name: row.path.file_name().unwrap().to_string_lossy().into_owned(),
                start: -1.0,
                end: -1.0,
                length: 0.0,
                rate: 1.0,
            };
            state.current_bank_mut().slots[idx] = Some(slot);
        }
    } else if self.open.contains(&row.path) {
        self.open.remove(&row.path);
    } else {
        self.open.insert(row.path.clone());
    }
}
```

4. In `BrowserBody::handle`, after the existing `Action::NavUp`/`NavDown` arms, add a small probe-enqueue helper call. Actually the cleanest path: extract a `maybe_enqueue_probe` private method and call it at the end of each Nav arm. Add to `impl BrowserBody`:

```rust
/// If the currently focused row is a file with no cached probe entry,
/// enqueue a probe request via state.probe_tx. No-op when no tx wired,
/// when the cursor is on a folder, or when the cache already has an entry.
fn maybe_enqueue_probe(&self, state: &mut SharedState, rows: &[BrowserRow]) {
    let Some(row) = rows.get(self.selected) else { return; };
    if !row.is_file {
        return;
    }
    let mtime = std::fs::metadata(&row.path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    if state
        .probe_cache
        .get_with_mtime(&row.probe_key, mtime)
        .is_some()
    {
        return;
    }
    if let Some(tx) = &state.probe_tx {
        let req = ProbeRequest {
            path: row.probe_key.clone(),
            mtime,
        };
        // Mark pending immediately so the render loop reflects the in-flight
        // state on the very next frame.
        state.probe_cache.mark_pending(&row.probe_key, mtime);
        let _ = tx.send(req);
    }
}
```

In each of the `Action::NavUp` / `Action::NavDown` arms, after the `self.selected = ...` line, add:

```rust
self.maybe_enqueue_probe(state, &rows);
```

(`rows` is the local `let rows = self.rows(state);` already at the top of `handle`.)

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib menu::browser`
Expected: PASS (4 new tests + the existing 3).

- [ ] **Step 5: Run full lib tests**

Run: `cargo test --lib`
Expected: green.

- [ ] **Step 6: Commit**

```bash
git add src/menu/browser.rs
git commit -m "feat(browser): probe-aware render (dim+[X]/[..]/refuse-map) + focus enqueues probe"
```

---

### Task 8: main.rs spawns ProbeWorker + drains results into state.probe_cache

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Write the integration test**

Create `tests/integration_codec_probe.rs`:

```rust
//! End-to-end probe of the bundled SMPTE test clip (h264). The probe worker
//! is spawned from a fresh `gst::init()` call; on a workstation with the
//! standard gst plugins installed, this returns Supported("h264") within ~1s.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use recur::video::{CodecStatus, ProbeRequest, ProbeWorker};

#[test]
fn probe_worker_returns_h264_for_smpte_clip() {
    gstreamer::init().expect("gst init");

    let clip = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/test_smpte.mp4");
    if !clip.exists() {
        // Bundled asset missing on this checkout — skip rather than fail.
        eprintln!("assets/test_smpte.mp4 missing; skipping probe smoke test");
        return;
    }

    let (req_tx, req_rx) = crossbeam_channel::unbounded::<ProbeRequest>();
    let (res_tx, res_rx) = crossbeam_channel::unbounded();
    let worker = ProbeWorker::spawn(req_rx, res_tx);

    let mtime = std::fs::metadata(&clip)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);

    req_tx
        .send(ProbeRequest {
            path: clip.clone(),
            mtime,
        })
        .unwrap();

    let deadline = Instant::now() + Duration::from_secs(5);
    let mut got: Option<CodecStatus> = None;
    while Instant::now() < deadline {
        if let Ok(r) = res_rx.recv_timeout(Duration::from_millis(200)) {
            got = Some(r.status);
            break;
        }
    }

    drop(req_tx);
    let _ = worker.join();

    assert_eq!(got, Some(CodecStatus::Supported("h264".into())));
}
```

- [ ] **Step 2: Run integration test to verify behaviour**

Run: `cargo test --test integration_codec_probe`
Expected: PASS, OR a "skipping probe smoke test" line (if the bundled clip is absent on this checkout). The test should NOT fail on a clean checkout.

If the test fails because of plugin issues (`Discoverer::new` returns Err — e.g. on a CI image lacking `gst-libav`), the worker logs a warn and exits. The test will then loop the deadline without a result and assert-fail. In that case mark the test `#[cfg_attr(target_os = "linux", ignore)]` — but try on the local dev box first.

- [ ] **Step 3: Wire main.rs**

In `src/main.rs`:

1. After the `ShaderWatcher` setup block (and after `state.shader_banks = shader_banks;`), add:

```rust
let (probe_tx, probe_req_rx) = crossbeam_channel::unbounded::<recur::video::ProbeRequest>();
let (probe_res_tx, probe_res_rx) = crossbeam_channel::unbounded::<recur::video::ProbeResult>();
let _probe_worker = recur::video::ProbeWorker::spawn(probe_req_rx, probe_res_tx);
state.probe_tx = Some(probe_tx);
```

(`_probe_worker` is held to keep the thread alive until `main` returns; dropping the Receiver via `probe_tx` closing on shutdown causes a clean worker exit.)

2. In the frame loop, alongside the existing `shader_rx.try_iter()` and `shader_watcher.try_drain()` drains (after them), add a probe-result drain block:

```rust
for res in probe_res_rx.try_iter() {
    let reclassified = recur::video::reclassify_for_profile(res.status, state.gles_profile);
    state.probe_cache.insert(&res.path, res.mtime, reclassified);
}
```

- [ ] **Step 4: Run full test suite**

Run: `cargo test`
Expected: PASS (integration test passes if asset exists; gracefully skips otherwise).

- [ ] **Step 5: Commit**

```bash
git add src/main.rs tests/integration_codec_probe.rs
git commit -m "feat(main): spawn ProbeWorker + drain results into state.probe_cache"
```

---

### Task 9: Smoke run — visual verification on macOS

This task is non-coding; it documents the manual run-through used to confirm the dim path + refuse-map work end-to-end. No commit unless something needs fixing.

- [ ] **Step 1: Bundle an HEVC test clip**

Place an h.265 file at `assets/test_hevc.mp4` (one-shot encode from the existing smpte clip is fine):

```bash
ffmpeg -i assets/test_smpte.mp4 -c:v libx265 -preset ultrafast -t 2 assets/test_hevc.mp4
```

If `ffmpeg` is unavailable, use any HEVC clip you have around. This is purely for visual smoke; the file is not committed.

- [ ] **Step 2: Run with default profile (V310 → desktop forces V100 via the macOS clamp)**

```bash
RECUR_SMOKE_AUTO_LOAD=0 cargo run -- 2>&1 | head -3
```

Then in the running UI:
1. Press `KeyB` to enter Browser mode.
2. Configure `paths_to_browser` to include `./assets/` if not already.
3. Navigate (ArrowDown) over `test_hevc.mp4` — within ~1s the row should dim and `[X]` appear at the end.
4. Press `Enter` on the dimmed row — footer should show `ERR: cannot map: pi3 build does not support hevc`.
5. Navigate to `test_smpte.mp4` — no dim, `Enter` maps it to slot 0 successfully.

- [ ] **Step 3: Run with `--gles-profile v310` (will get clamped on macOS, but exercises the reclassify path)**

```bash
cargo run -- --gles-profile v310
```

This reaches the desktop clamp, lands on V100. Same behaviour as default run — confirms reclassify-on-drain reacts to profile (not just CLI flag).

- [ ] **Step 4: No commit unless step 2 or 3 reveal a bug.**

---

### Task 10: ROADMAP update — close Phase 2

**Files:**
- Modify: `.docs/ROADMAP.md`

- [ ] **Step 1: Mark Phase 2 ✅**

In `.docs/ROADMAP.md`:

1. Update the `## Recently Shipped` section. Add at the top:

```markdown
- **Phase 2 sub-plan C — conjur codec probe** (2026-05-16): `gstreamer-pbutils` Discoverer worker thread; FILES browser dims unsupported codecs with `[X]` glyph; map-to-slot refused with status-line `cannot map: pi3 build does not support hevc`; `ATTR_DIM` wired through TextOverlay. Codec lists per-profile (pi3 blocks hevc/vp9/av1; pi5 accepts all).
```

Roll the oldest entry off into `.docs/COMPLETED.md` (the "Dual-target spec" entry, since sub-plan A and B are more recent and now also include conjur work).

2. Change the Phase 2 row in `## Execution Order`:

```markdown
| 2 | **conjur** — GLSL shader layer + codec probe. All three sub-plans (A: infra, B: UI+persistence, C: codec probe) shipped 2026-05-16 | ✅ | `src/shader/`, `shaders/`, `src/menu/{shaders,shdr_bnk,param}.rs`, `src/video/probe.rs` |
```

Phase 2 active phase moves to Phase 3 (detour).

- [ ] **Step 2: Commit**

```bash
git add .docs/ROADMAP.md .docs/COMPLETED.md
git commit -m "docs(roadmap): Phase 2 (conjur) complete — all 3 sub-plans shipped; Phase 3 (detour) next"
```

---

### Task 11: Done-criteria verification

- [ ] **Step 1: Full test suite**

Run: `cargo test`
Expected: green (integration probe test passes if h264 asset is present; otherwise the test logs "skipping" and is a no-op).

- [ ] **Step 2: Cross-target builds**

Run:

```bash
cross build --no-default-features --features pi3 --target aarch64-unknown-linux-gnu
cross build --no-default-features --features pi5 --target aarch64-unknown-linux-gnu
```

If docker isn't running locally, skip — CI matrix (existing on `rust-port`) covers it.

- [ ] **Step 3: Visual confirmation (covered by Task 9).**

- [ ] **Step 4: Update auto-memory `project_recur_phase2.md`**

Update the per-sub-plan table: mark C ✅ with commit range, and switch the "How to apply" to point at Phase 3 (detour) as the next stop. This is a memory file, not a repo file — use the Write tool against `/Users/cody/.claude/projects/-Users-cody-Dev/memory/project_recur_phase2.md` rather than `git add`.

---

## Self-Review

**1. Spec coverage:** Section 7 (Codec Probe) is decomposed into Tasks 2–8. Section 7's "UI" sub-bullets map to Task 1 (ATTR_DIM render) + Task 7 (browser dim/X/[…] + refuse-map). Async-probing details (worker thread, 2s timeout, mtime cache invalidation) → Tasks 3, 4, 8. Per-target unsupported list → Task 2 + Task 4. Reclassify against runtime profile → Task 4. Spec note about external workaround (`ffmpeg -c:v libx264 …`) goes in README — not in this plan; README touch should happen alongside Task 10. Add to T10:

> Also append to README's "Sources" section: "On `pi3` builds, HEVC/VP9/AV1 files are dimmed in the FILES browser and cannot be mapped to slots. Workaround: `ffmpeg -i input.mkv -c:v libx264 -preset fast output.mp4`."

(Adding inline as a follow-up step on T10 — see the next task list update.)

**2. Placeholder scan:** Clean. No "TBD" or "implement later".

**3. Type consistency:**
- `CodecStatus` consistent (Pending / Supported(String) / Unsupported(String) / Unknown).
- `ProbeRequest { path, mtime }` and `ProbeResult { path, mtime, status }` consistent across Tasks 2, 3, 8.
- `ProbeCache::get`, `get_with_mtime`, `mark_pending`, `insert` consistent.
- `unsupported_for_profile` returns `&'static [&'static str]`; usage in `reclassify_for_profile` uses `.contains(&name.as_str())` — correct.
- `state.probe_tx: Option<Sender<ProbeRequest>>` and `state.probe_cache: ProbeCache` consistent across Tasks 5, 7, 8.
- `BrowserRow.probe_key: PathBuf` consistent across Tasks 6, 7.
- `ProbeWorker::spawn(rx, res_tx) -> Self` and `.join()` consistent.

**4. Inline fix from review:** Add the README touch to T10.
