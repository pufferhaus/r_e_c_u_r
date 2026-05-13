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
}

impl Player {
    pub fn empty(layer: u32) -> Self {
        Self {
            status: PlayerStatus::Empty,
            slot: None,
            layer,
            alpha: 0.0,
            last_position: 0.0,
            last_error: None,
            pipeline: None,
            appsink: None,
        }
    }

    pub fn try_load(&mut self, slot: Slot) -> Result<()> {
        self.unload();
        let BuiltPipeline { pipeline, appsink } =
            pipeline_factory::build_for_file(&slot.location)?;
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
                Eos(_) => self.status = PlayerStatus::Finished,
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
        let p = Player::empty(0);
        assert_eq!(p.status, PlayerStatus::Empty);
        assert!(p.slot.is_none());
    }

    #[test]
    #[ignore] // requires gstreamer plugins installed
    fn loads_test_clip_to_loaded_state() {
        init_gst();
        let mut p = Player::empty(0);
        let slot = Slot {
            location: test_clip(),
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
        let mut p = Player::empty(0);
        let slot = Slot {
            location: test_clip(),
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
