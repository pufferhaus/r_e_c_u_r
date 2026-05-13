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

/// Build `uridecodebin uri=file://… ! videoconvert ! video/x-raw,format=RGBA !
/// appsink`. We use a plain appsink (CPU path) in Phase 1 — the GL upload
/// happens in our render module via `glTexSubImage2D`. Future tasks can
/// swap this for `glupload ! glsinkbin` once the GL context is wired through.
pub fn build_for_file(path: &Path) -> Result<BuiltPipeline> {
    let abs = path.canonicalize().map_err(Error::from)?;
    let uri = gst::glib::filename_to_uri(&abs, None)
        .map_err(|e| Error::Gst(format!("filename_to_uri: {e}")))?;

    let desc = format!(
        "uridecodebin uri={uri} ! videoconvert ! video/x-raw,format=RGBA ! \
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
