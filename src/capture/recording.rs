//! Phase 4b — capture recording state + helpers.

use std::path::{Path, PathBuf};
use std::time::Instant;

/// Build target for encoder selection. Inferred at compile time by `Target::current()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Pi3,
    Pi5,
    /// macOS desktop — uses VideoToolbox vtenc_h264.
    MacDesktop,
    /// Linux desktop — uses software x264enc.
    LinuxDesktop,
}

impl Target {
    pub fn current() -> Self {
        #[cfg(feature = "pi3")]
        {
            return Target::Pi3;
        }
        #[cfg(feature = "pi5")]
        {
            return Target::Pi5;
        }
        #[cfg(all(not(feature = "pi3"), not(feature = "pi5"), target_os = "macos"))]
        {
            return Target::MacDesktop;
        }
        #[cfg(all(not(feature = "pi3"), not(feature = "pi5"), target_os = "linux"))]
        {
            return Target::LinuxDesktop;
        }
        #[cfg(all(
            not(feature = "pi3"),
            not(feature = "pi5"),
            not(target_os = "macos"),
            not(target_os = "linux")
        ))]
        {
            return Target::LinuxDesktop;
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecState {
    Recording,
    Finalizing,
}

#[derive(Debug, Clone)]
pub struct ActiveRecording {
    pub device_path: String,
    pub file_path: PathBuf,
    pub started_at: Instant,
    pub state: RecState,
    pub last_disk_check: Instant,
}

/// Returns the first non-colliding path `<dir>/rec-YYYY-MM-DD-N.mp4`.
/// `N` starts at 0 and increments until the candidate does not exist.
/// Pure of side-effects except for `Path::exists()` checks.
///
/// `date_yyyymmdd` is injected so tests don't depend on system clock.
pub fn generate_recording_path(dir: &Path, date_yyyymmdd: &str) -> PathBuf {
    let mut n = 0u32;
    loop {
        let candidate = dir.join(format!("rec-{date_yyyymmdd}-{n}.mp4"));
        if !candidate.exists() {
            return candidate;
        }
        n += 1;
    }
}

/// Returns true if `dir` (or its nearest existing ancestor) has at least
/// `min_mb` megabytes of free space. On platforms or filesystems where
/// `statvfs` fails, returns `true` (fail-open) to avoid blocking
/// recording on unreliable stats.
pub fn check_disk_space(dir: &Path, min_mb: u64) -> bool {
    // Walk up to the nearest existing dir for the stat call.
    let mut probe = dir.to_path_buf();
    while !probe.exists() {
        match probe.parent() {
            Some(p) => probe = p.to_path_buf(),
            None => return true, // can't resolve any ancestor — fail-open
        }
    }
    match free_mb(&probe) {
        Some(mb) => mb >= min_mb,
        None => true, // statvfs failed — fail-open
    }
}

#[cfg(unix)]
fn free_mb(dir: &Path) -> Option<u64> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let c = CString::new(dir.as_os_str().as_bytes()).ok()?;
    let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
    let rc = unsafe { libc::statvfs(c.as_ptr(), &mut stat) };
    if rc != 0 {
        return None;
    }
    let bsize = stat.f_frsize as u64;
    let avail = stat.f_bavail as u64;
    Some((bsize.saturating_mul(avail)) / (1024 * 1024))
}

#[cfg(not(unix))]
fn free_mb(_dir: &Path) -> Option<u64> {
    None
}

/// Builds the gst-parse-launch string for the record-branch bin. This is the
/// content of the second tee output: `queue ! <encoder> ! <parser> ! splitmuxsink`.
///
/// The caller wraps this in a `Bin` and links it to the live capture pipeline's
/// `cap_t` tee request pad.
pub fn build_record_bin_desc(target: Target, file_path: &Path) -> String {
    let (encoder, parser) = encoder_chain(target);
    let location = file_path.display();
    format!(
        "queue ! {encoder} ! {parser} ! \
         splitmuxsink muxer-factory=mp4mux max-size-time=0 location=\"{location}\""
    )
}

fn encoder_chain(target: Target) -> (&'static str, &'static str) {
    match target {
        Target::Pi3 => ("v4l2h264enc", "h264parse"),
        Target::Pi5 => ("v4l2h265enc", "h265parse"),
        Target::MacDesktop => ("vtenc_h264", "h264parse"),
        Target::LinuxDesktop => ("x264enc", "h264parse"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn generate_recording_path_starts_at_zero() {
        let td = TempDir::new().unwrap();
        let p = generate_recording_path(td.path(), "2026-05-17");
        assert_eq!(
            p.file_name().unwrap().to_str(),
            Some("rec-2026-05-17-0.mp4")
        );
    }

    #[test]
    fn generate_recording_path_increments_on_collision() {
        let td = TempDir::new().unwrap();
        fs::write(td.path().join("rec-2026-05-17-0.mp4"), b"").unwrap();
        fs::write(td.path().join("rec-2026-05-17-1.mp4"), b"").unwrap();
        let p = generate_recording_path(td.path(), "2026-05-17");
        assert_eq!(
            p.file_name().unwrap().to_str(),
            Some("rec-2026-05-17-2.mp4")
        );
    }

    #[test]
    fn target_current_is_one_of_known() {
        let t = Target::current();
        // Just verify it compiles and resolves to a known variant.
        match t {
            Target::Pi3 | Target::Pi5 | Target::MacDesktop | Target::LinuxDesktop => {}
        }
    }

    #[test]
    fn rec_state_distinct() {
        assert_ne!(RecState::Recording, RecState::Finalizing);
    }

    #[test]
    fn active_recording_round_trip_fields() {
        let now = Instant::now();
        let r = ActiveRecording {
            device_path: "/dev/video0".into(),
            file_path: "/tmp/rec.mp4".into(),
            started_at: now,
            state: RecState::Recording,
            last_disk_check: now,
        };
        assert_eq!(r.device_path, "/dev/video0");
        assert_eq!(r.state, RecState::Recording);
    }

    #[test]
    fn check_disk_space_passes_for_zero_threshold_on_tempdir() {
        let td = TempDir::new().unwrap();
        assert!(check_disk_space(td.path(), 0));
    }

    #[test]
    fn check_disk_space_fail_opens_when_path_does_not_exist() {
        // Walks up to root, which exists — but we expect the function to
        // resolve and return true (free or fail-open).
        let p = std::path::Path::new("/nonexistent-r_e_c_u_r-test-dir-zzz");
        assert!(check_disk_space(p, 0));
    }

    #[test]
    fn record_bin_desc_pi3_uses_v4l2h264enc() {
        let d = build_record_bin_desc(Target::Pi3, Path::new("/tmp/r.mp4"));
        assert!(d.contains("v4l2h264enc"));
        assert!(d.contains("h264parse"));
        assert!(d.contains("splitmuxsink"));
        assert!(d.contains("muxer-factory=mp4mux"));
        assert!(d.contains("max-size-time=0"));
        assert!(
            d.contains("location=\"/tmp/r.mp4\""),
            "missing quoted location: {d}"
        );
    }

    #[test]
    fn record_bin_desc_pi5_uses_v4l2h265enc() {
        let d = build_record_bin_desc(Target::Pi5, Path::new("/tmp/r.mp4"));
        assert!(d.contains("v4l2h265enc"));
        assert!(d.contains("h265parse"));
    }

    #[test]
    fn record_bin_desc_mac_uses_vtenc_h264() {
        let d = build_record_bin_desc(Target::MacDesktop, Path::new("/tmp/r.mp4"));
        assert!(d.contains("vtenc_h264"));
        assert!(d.contains("h264parse"));
    }

    #[test]
    fn record_bin_desc_linux_desktop_uses_x264enc() {
        let d = build_record_bin_desc(Target::LinuxDesktop, Path::new("/tmp/r.mp4"));
        assert!(d.contains("x264enc"));
        assert!(d.contains("h264parse"));
    }
}
