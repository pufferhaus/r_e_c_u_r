//! Build the GStreamer pipeline used by each Player.

use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app::AppSink;
use std::path::Path;

use crate::error::{Error, Result};

pub struct BuiltPipeline {
    pub pipeline: gst::Pipeline,
    pub appsink: AppSink,
}

/// Build `uridecodebin uri=file://… ! videoconvert ! videoscale !
/// video/x-raw,format=RGBA,width=W,height=H ! appsink`. Frames are scaled to
/// the render resolution so the detour ring never rejects size-mismatched frames.
pub fn build_for_file(path: &Path, render_width: u32, render_height: u32) -> Result<BuiltPipeline> {
    let abs = path.canonicalize().map_err(Error::from)?;
    let uri = gst::glib::filename_to_uri(&abs, None)
        .map_err(|e| Error::Gst(format!("filename_to_uri: {e}")))?;

    let desc = format!(
        "uridecodebin uri={uri} ! videoconvert ! videoscale ! \
         video/x-raw,format=RGBA,width={render_width},height={render_height} ! \
         appsink name=sink sync=true max-buffers=2 drop=true emit-signals=false",
    );

    let pipeline = gst::parse::launch(&desc)
        .map_err(|e| Error::Gst(format!("launch: {e}")))?
        .downcast::<gst::Pipeline>()
        .map_err(|_| Error::Gst("not a Pipeline".into()))?;

    let appsink = pipeline
        .by_name("sink")
        .ok_or_else(|| Error::Gst("no appsink".into()))?
        .downcast::<AppSink>()
        .map_err(|_| Error::Gst("appsink downcast".into()))?;

    Ok(BuiltPipeline { pipeline, appsink })
}

use crate::capture::CaptureDevice;

/// Build a live-capture pipeline. On a smoke test or CI without hardware,
/// `gst::parse::launch` may still succeed (pipeline description is syntactically
/// valid); errors surface only when state is set to Playing.
pub fn build_for_capture(device: &CaptureDevice, w: u32, h: u32) -> Result<BuiltPipeline> {
    let desc = capture_pipeline_desc(device, w, h);
    let pipeline = gst::parse::launch(&desc)
        .map_err(|e| Error::Gst(format!("launch capture: {e}")))?
        .downcast::<gst::Pipeline>()
        .map_err(|_| Error::Gst("capture not a Pipeline".into()))?;
    let appsink = pipeline
        .by_name("sink")
        .ok_or_else(|| Error::Gst("no appsink in capture pipeline".into()))?
        .downcast::<AppSink>()
        .map_err(|_| Error::Gst("appsink downcast".into()))?;
    Ok(BuiltPipeline { pipeline, appsink })
}

pub(crate) fn capture_pipeline_desc(device: &CaptureDevice, w: u32, h: u32) -> String {
    let source = capture_source_element(device);
    format!(
        "{source} ! videoconvert ! tee name=cap_t \
         cap_t. ! queue ! videoscale ! \
           video/x-raw,format=RGBA,width={w},height={h} ! \
           appsink name=sink sync=true max-buffers=2 drop=true emit-signals=false \
         cap_t. ! queue ! fakesink name=rec_placeholder sync=false"
    )
}

#[cfg(target_os = "linux")]
pub(crate) fn capture_source_element(d: &CaptureDevice) -> String {
    format!("v4l2src device={}", d.path)
}

#[cfg(target_os = "macos")]
pub(crate) fn capture_source_element(d: &CaptureDevice) -> String {
    let idx = d.path.parse::<u32>().unwrap_or(0);
    format!("avfvideosrc device-index={idx}")
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub(crate) fn capture_source_element(_d: &CaptureDevice) -> String {
    "fakesrc".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::CaptureDevice;

    #[test]
    #[cfg(target_os = "linux")]
    fn capture_source_element_linux_uses_v4l2src() {
        let d = CaptureDevice {
            path: "/dev/video0".into(),
            label: "v4l2:video0".into(),
        };
        let s = capture_source_element(&d);
        assert!(s.contains("v4l2src"));
        assert!(s.contains("/dev/video0"));
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn capture_source_element_macos_uses_avfvideosrc() {
        let d = CaptureDevice {
            path: "0".into(),
            label: "avf-camera-0".into(),
        };
        let s = capture_source_element(&d);
        assert!(s.contains("avfvideosrc"));
        assert!(s.contains("device-index=0"));
    }

    #[test]
    fn capture_pipeline_desc_has_tee_and_appsink_and_fakesink() {
        let d = CaptureDevice {
            path: "/dev/video0".into(),
            label: "v4l2:video0".into(),
        };
        let desc = capture_pipeline_desc(&d, 720, 480);
        assert!(desc.contains("tee name=cap_t"), "missing tee: {desc}");
        assert!(desc.contains("videoscale"));
        assert!(desc.contains("width=720,height=480"));
        assert!(desc.contains("appsink"));
        // Second branch — placeholder so record bin can replace it later.
        assert!(
            desc.contains("fakesink"),
            "missing placeholder fakesink: {desc}"
        );
    }
}
