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
