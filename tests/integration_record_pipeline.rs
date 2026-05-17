//! Phase 4b — gst integration smoke test for the record bin description.
//!
//! Uses `videotestsrc` as the source so no real camera is needed. This is a
//! plugin-availability smoke: it validates that the `build_record_bin_desc`
//! string parses, runs end-to-end inside `parse::launch`, and produces a
//! non-empty MP4. It does NOT exercise `Player::start_recording`'s dynamic
//! `pipeline.add(bin) + tee.request_pad + link` path — that requires hardware
//! and is documented as hardware-pending in the Phase 4b spec.

use gstreamer as gst;
use gstreamer::prelude::*;
use recur::capture::recording::{build_record_bin_desc, Target};
use std::path::PathBuf;
use std::time::Duration;

fn init() {
    let _ = gst::init();
}

#[test]
#[ignore = "requires gst-plugins-good (videotestsrc, splitmuxsink) + ugly (x264enc) or bad (vtenc_h264)"]
fn videotestsrc_records_one_second_mp4() {
    init();
    let dir = tempfile::TempDir::new().unwrap();
    let path: PathBuf = dir.path().join("rec.mp4");
    let target = if cfg!(target_os = "macos") {
        Target::MacDesktop
    } else {
        Target::LinuxDesktop
    };

    let rec_desc = build_record_bin_desc(target, &path);
    let pipeline_desc = format!(
        "videotestsrc num-buffers=30 ! videoconvert ! tee name=cap_t \
         cap_t. ! queue ! fakesink sync=false \
         cap_t. ! {rec_desc}"
    );

    let pipeline = gst::parse::launch(&pipeline_desc)
        .expect("parse")
        .downcast::<gst::Pipeline>()
        .expect("Pipeline");
    pipeline.set_state(gst::State::Playing).unwrap();

    // Drain bus until EOS or 5s timeout.
    let bus = pipeline.bus().unwrap();
    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_secs(5) {
        if let Some(msg) = bus.timed_pop(gst::ClockTime::from_mseconds(100)) {
            use gst::MessageView::*;
            match msg.view() {
                Eos(_) => break,
                Error(e) => panic!("gst error: {}", e.error()),
                _ => {}
            }
        }
    }
    pipeline.set_state(gst::State::Null).unwrap();

    assert!(path.exists(), "MP4 not written: {path:?}");
    let meta = std::fs::metadata(&path).unwrap();
    assert!(meta.len() > 0, "MP4 has zero bytes");
}
