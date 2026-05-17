//! Single playback pipeline + state machine.

use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app::AppSink;
use tracing::warn;

use crate::error::Result;
use crate::state::Slot;
use crate::video::pipeline_factory::{self, BuiltPipeline};

/// A decoded video frame that borrows directly from the GStreamer buffer — no
/// per-frame copy. The `MappedBuffer` holds both the buffer ref and the map
/// lock; it is unmapped on drop. The sample is retained to keep the buffer's
/// refcount alive for the duration of the frame.
pub struct VideoFrame {
    /// Keeps the buffer refcount alive for the duration of the map.
    _sample: gst::Sample,
    map: gst::buffer::MappedBuffer<gst::buffer::Readable>,
    pub width: u32,
    pub height: u32,
}

impl VideoFrame {
    /// Returns a zero-copy view of the decoded RGBA pixels.
    pub fn data(&self) -> &[u8] {
        self.map.as_slice()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerStatus {
    Empty,
    Loading,
    Loaded,
    Playing,
    Paused,
    Finished,
    Error,
}

pub struct Player {
    pub status: PlayerStatus,
    pub slot: Option<Slot>,
    pub layer: u32,
    pub alpha: f32,
    pub last_position: f64,
    pub last_error: Option<String>,
    pipeline: Option<gst::Pipeline>,
    pub appsink: Option<AppSink>,
    pub render_width: u32,
    pub render_height: u32,
    /// Phase 4b — the record bin currently attached to the capture pipeline's
    /// tee, or `None` when not recording.
    pub recording_bin: Option<gst::Bin>,
    /// Phase 4b — the output file path the active recording is writing to.
    /// Set in `start_recording`, cleared on finalize. Used by `stop_recording_self`
    /// so we don't have to fish it back out of the bin's properties.
    pub recording_path: Option<std::path::PathBuf>,
    /// Phase 4b — when the record bin sent EOS and tear-down completed,
    /// the finalized file path is pushed here. Drained by the rack.
    pub finalized_records: Vec<std::path::PathBuf>,
    /// Phase 4b — (bin awaiting EOS round-trip, file path to publish on success).
    pub pending_finalize: Option<(gst::Bin, std::path::PathBuf)>,
}

impl Player {
    pub fn empty(layer: u32, render_width: u32, render_height: u32) -> Self {
        Self {
            status: PlayerStatus::Empty,
            slot: None,
            layer,
            alpha: 0.0,
            last_position: 0.0,
            last_error: None,
            pipeline: None,
            appsink: None,
            render_width,
            render_height,
            recording_bin: None,
            recording_path: None,
            finalized_records: Vec::new(),
            pending_finalize: None,
        }
    }

    pub fn try_load(&mut self, slot: Slot) -> Result<()> {
        self.unload();
        let BuiltPipeline { pipeline, appsink } = match &slot.source {
            crate::state::SourceKind::File(p) => {
                pipeline_factory::build_for_file(p, self.render_width, self.render_height)?
            }
            crate::state::SourceKind::Capture(d) => {
                pipeline_factory::build_for_capture(d, self.render_width, self.render_height)?
            }
        };
        pipeline.set_state(gst::State::Paused).map_err(|e| {
            crate::Error::Gst(format!("set_state Paused: {e}"))
        })?;
        self.pipeline = Some(pipeline);
        self.appsink = Some(appsink);
        self.slot = Some(slot);
        self.status = PlayerStatus::Loading;
        Ok(())
    }

    /// Pump bus. Call once per main-loop tick. Transitions Loading→Loaded on
    /// `AsyncDone`, sets `Error` on `Error`, and clears `Finished` on `Eos`.
    pub fn tick(&mut self) {
        let Some(pipeline) = self.pipeline.clone() else {
            return;
        };
        let bus = pipeline.bus().expect("pipeline has bus");
        while let Some(msg) = bus.pop() {
            use gst::MessageView::*;
            match msg.view() {
                AsyncDone(_) if self.status == PlayerStatus::Loading => {
                    self.seek_to_start();
                    self.status = PlayerStatus::Loaded;
                }
                Eos(_) => {
                    // EOS from the record bin is distinct from EOS of the
                    // source. Disambiguate by checking whether the message
                    // source has the pending record bin as an ancestor.
                    let from_record_bin = match (msg.src(), self.pending_finalize.as_ref()) {
                        (Some(src), Some((bin, _))) => src.has_as_ancestor(bin),
                        _ => false,
                    };
                    if from_record_bin {
                        if let Some((bin, path)) = self.pending_finalize.take() {
                            let _ = bin.set_state(gst::State::Null);
                            let _ = pipeline.remove(&bin);
                            // Re-attach the placeholder fakesink so the tee
                            // never has an unlinked request pad.
                            if let (Some(ph), Some(tee)) = (
                                pipeline.by_name("rec_placeholder"),
                                pipeline.by_name("cap_t"),
                            ) {
                                let _ = ph.sync_state_with_parent();
                                if let (Some(tee_src), Some(ph_sink)) = (
                                    tee.request_pad_simple("src_%u"),
                                    ph.static_pad("sink"),
                                ) {
                                    if ph_sink.peer().is_none() {
                                        let _ = tee_src.link(&ph_sink);
                                    } else {
                                        // Already linked from before — release
                                        // the spurious extra request pad.
                                        tee.release_request_pad(&tee_src);
                                    }
                                }
                            }
                            self.recording_path = None;
                            self.finalized_records.push(path);
                        }
                    } else {
                        self.status = PlayerStatus::Finished;
                    }
                }
                Error(e) => {
                    let msg = format!("gst error from {:?}: {}", e.src().map(|s| s.name()), e.error());
                    warn!("{msg}");
                    self.last_error = Some(msg);
                    self.status = PlayerStatus::Error;
                }
                _ => {}
            }
        }
        // Update position cache.
        if let Some(p) = pipeline.query_position::<gst::ClockTime>() {
            self.last_position = p.seconds_f64();
        }
        // Honour user-defined loop-out.
        if let Some(slot) = &self.slot {
            if slot.end > 0.0 && self.last_position >= slot.end {
                self.status = PlayerStatus::Finished;
            }
        }
    }

    pub fn play(&mut self) {
        if let Some(p) = &self.pipeline {
            let _ = p.set_state(gst::State::Playing);
            self.status = PlayerStatus::Playing;
        }
    }

    pub fn pause(&mut self) {
        if let Some(p) = &self.pipeline {
            let _ = p.set_state(gst::State::Paused);
            self.status = PlayerStatus::Paused;
        }
    }

    pub fn unload(&mut self) {
        // Tear down any in-flight record bin before tearing down the parent
        // pipeline so we don't leak the bin's state or its sink file handle.
        if let Some(bin) = self.recording_bin.take() {
            let _ = bin.set_state(gst::State::Null);
        }
        self.pending_finalize = None;
        self.recording_path = None;
        if let Some(p) = self.pipeline.take() {
            let _ = p.set_state(gst::State::Null);
        }
        self.appsink = None;
        self.slot = None;
        self.status = PlayerStatus::Empty;
        self.last_position = 0.0;
    }

    /// Pull the most recent decoded RGBA frame from the appsink, if one is
    /// available right now. Returns `None` when no sample is ready or the
    /// pipeline is not in a playing/paused state.
    ///
    /// The returned `VideoFrame` holds a zero-copy view into the GStreamer
    /// buffer — no allocation or memcpy. The buffer is unmapped when the
    /// frame is dropped.
    pub fn pull_latest_rgba(&self) -> Option<VideoFrame> {
        let appsink = self.appsink.as_ref()?;
        let sample = appsink.try_pull_sample(gst::ClockTime::ZERO)?;
        let buffer = sample.buffer_owned()?;
        let caps = sample.caps()?;
        let s = caps.structure(0)?;
        let w: i32 = s.get("width").ok()?;
        let h: i32 = s.get("height").ok()?;
        let map = buffer.into_mapped_buffer_readable().ok()?;
        Some(VideoFrame { _sample: sample, map, width: w as u32, height: h as u32 })
    }

    /// Phase 4b — attach a record bin to the live capture pipeline's `cap_t`
    /// tee. Detaches the placeholder fakesink first (releasing its request
    /// pad) and links a fresh tee src pad to the record bin's sink ghost.
    pub fn start_recording(
        &mut self,
        file_path: &std::path::Path,
        target: crate::capture::recording::Target,
    ) -> Result<()> {
        if self.recording_bin.is_some() {
            return Err(crate::Error::Gst(
                "already recording on this player".into(),
            ));
        }
        let pipeline = self
            .pipeline
            .as_ref()
            .ok_or_else(|| crate::Error::Gst("player has no pipeline".into()))?;
        let tee = pipeline.by_name("cap_t").ok_or_else(|| {
            crate::Error::Gst("no `cap_t` tee on pipeline (not a capture player?)".into())
        })?;
        let placeholder = pipeline.by_name("rec_placeholder");

        // Build the record bin from the parse description. The parse helper
        // auto-ghosts the leading `queue ! ...` chain's sink pad so we can
        // link the tee straight to `bin.static_pad("sink")`.
        let desc = crate::capture::recording::build_record_bin_desc(target, file_path);
        let bin = gst::parse::bin_from_description(&desc, /*ghost_unlinked_pads=*/ true)
            .map_err(|e| crate::Error::Gst(format!("bin_from_description: {e}")))?;

        pipeline
            .add(&bin)
            .map_err(|e| crate::Error::Gst(format!("pipeline.add(bin): {e}")))?;
        bin.sync_state_with_parent()
            .map_err(|e| crate::Error::Gst(format!("sync_state_with_parent: {e}")))?;

        // Detach the placeholder fakesink: unlink its sink pad from its tee
        // peer, release the tee's request pad, and put the fakesink to NULL
        // so it stops draining buffers but remains in the pipeline ready to
        // be re-attached when recording stops.
        if let Some(ph) = placeholder.as_ref() {
            if let Some(sink_pad) = ph.static_pad("sink") {
                if let Some(peer) = sink_pad.peer() {
                    let _ = peer.unlink(&sink_pad);
                    if let Some(tee_ref) = peer.parent_element() {
                        tee_ref.release_request_pad(&peer);
                    }
                }
            }
            let _ = ph.set_state(gst::State::Null);
        }

        // Request a fresh src pad on the tee and link to the record bin's
        // sink ghost.
        let tee_src = tee
            .request_pad_simple("src_%u")
            .ok_or_else(|| crate::Error::Gst("tee.request_pad failed".into()))?;
        let bin_sink = bin
            .static_pad("sink")
            .ok_or_else(|| crate::Error::Gst("record bin has no sink ghost".into()))?;
        tee_src
            .link(&bin_sink)
            .map_err(|e| crate::Error::Gst(format!("tee->bin link: {e:?}")))?;

        self.recording_bin = Some(bin);
        self.recording_path = Some(file_path.to_path_buf());
        Ok(())
    }

    /// Phase 4b — send EOS to the attached record bin. Tear-down completes
    /// asynchronously; the finalized path lands in `finalized_records` once
    /// the bus reports EOS forwarded from the bin.
    pub fn stop_recording(&mut self, file_path: std::path::PathBuf) {
        let Some(bin) = self.recording_bin.take() else {
            return;
        };
        if let Some(sink) = bin.static_pad("sink") {
            let _ = sink.send_event(gst::event::Eos::new());
        }
        self.pending_finalize = Some((bin, file_path));
    }

    /// Phase 4b — convenience: stop recording using whatever file path the
    /// player attached at `start_recording` time.
    pub fn stop_recording_self(&mut self) {
        let path = self
            .recording_path
            .take()
            .unwrap_or_else(|| std::path::PathBuf::from("recording.mp4"));
        self.stop_recording(path);
    }

    fn seek_to_start(&mut self) {
        let Some(p) = &self.pipeline else { return };
        let Some(slot) = &self.slot else { return };
        let start_s = if slot.start > 0.0 { slot.start } else { 0.0 };
        let pos = gst::ClockTime::from_seconds_f64(start_s);
        let _ = p.seek_simple(
            gst::SeekFlags::FLUSH | gst::SeekFlags::ACCURATE,
            pos,
        );
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        self.unload();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn init_gst() {
        let _ = gst::init();
    }

    fn test_clip() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/test_smpte.mp4")
    }

    #[test]
    fn empty_player_has_empty_status() {
        let p = Player::empty(0, 720, 480);
        assert_eq!(p.status, PlayerStatus::Empty);
        assert!(p.slot.is_none());
    }

    #[test]
    #[ignore] // requires gstreamer plugins installed
    fn loads_test_clip_to_loaded_state() {
        init_gst();
        let mut p = Player::empty(0, 720, 480);
        let slot = Slot {
            source: crate::state::SourceKind::File(test_clip()),
            name: "test_smpte.mp4".into(),
            start: -1.0,
            end: -1.0,
            length: 0.0,
            rate: 1.0,
        };
        p.try_load(slot).expect("load");
        for _ in 0..200 {
            p.tick();
            if p.status == PlayerStatus::Loaded {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("never reached Loaded, ended at {:?}", p.status);
    }

    #[test]
    #[ignore] // requires gst plugins + bundled clip
    fn pulls_an_rgba_frame_after_load() {
        init_gst();
        let mut p = Player::empty(0, 720, 480);
        let slot = Slot {
            source: crate::state::SourceKind::File(test_clip()),
            name: "test_smpte.mp4".into(),
            start: -1.0,
            end: -1.0,
            length: 0.0,
            rate: 1.0,
        };
        p.try_load(slot).unwrap();
        for _ in 0..200 {
            p.tick();
            if p.status == PlayerStatus::Loaded {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        p.play();
        for _ in 0..200 {
            p.tick();
            if let Some(frame) = p.pull_latest_rgba() {
                assert_eq!(frame.data().len(), (frame.width * frame.height * 4) as usize);
                assert_eq!(frame.width, 720);
                assert_eq!(frame.height, 480);
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("never pulled an RGBA sample");
    }
}
