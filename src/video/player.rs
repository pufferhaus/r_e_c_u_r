//! Single playback pipeline + state machine.

use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app::AppSink;
use tracing::warn;

use crate::error::Result;
use crate::state::Slot;
use crate::video::pipeline_factory::{self, BuiltPipeline};

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
                    warn!("gst error from {:?}: {}", e.src().map(|s| s.name()), e.error());
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
}
