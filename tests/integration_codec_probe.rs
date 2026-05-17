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
