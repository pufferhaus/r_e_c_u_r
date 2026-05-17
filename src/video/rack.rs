//! Three-pipeline rotation: `last / current / next`. Hides decode latency.

use crossbeam_channel::Sender;
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;
use tracing::warn;

#[derive(Debug, Clone)]
pub enum ShaderCommand {
    Trigger(String, [f32; 8]),
    SetParams([f32; 8]),
    Clear,
}

#[derive(Debug, Clone)]
pub enum DetourCommand {
    ScrubBy(i32),
}

use crate::apply::RackHandle;
use crate::error::Result;
use crate::state::{Bank, LoopType, OnFinish, SamplerSettings, Slot};
use crate::video::player::{Player, PlayerStatus};

pub struct PlayerRack {
    pub last: Player,
    pub current: Player,
    pub next: Player,
    pub settings: SamplerSettings,
    pub rng: ChaCha8Rng,
    /// Monotonically decreasing layer counter. Wraps at 0 → 254.
    pub next_layer: u32,
    /// Which (bank, slot_index) the current player was explicitly triggered on.
    /// Cleared on swap so loop-point edits only apply to the actively triggered player.
    /// Future work: track next_binding separately and promote on swap.
    pub current_binding: Option<(u8, u8)>,
    /// Snapshot of the bank that was active at the last `trigger_slot_with` call.
    /// Used by `tick_sequential` to pre-queue the successor slot so that
    /// `OnFinish::Switch` can fire a decode-free swap.
    pub last_bank: Bank,
    /// Channel to the render thread for shader commands. Set by main.rs after
    /// the channel pair is created. None until wired up.
    shader_tx: Option<Sender<ShaderCommand>>,
    /// Channel to the main loop for detour commands. Set by main.rs after
    /// the channel pair is created. None until wired up.
    detour_tx: Option<Sender<DetourCommand>>,
}

impl PlayerRack {
    pub fn new(settings: SamplerSettings, render_width: u32, render_height: u32) -> Self {
        Self {
            last: Player::empty(254, render_width, render_height),
            current: Player::empty(253, render_width, render_height),
            next: Player::empty(252, render_width, render_height),
            settings,
            rng: ChaCha8Rng::seed_from_u64(0xC0FFEE),
            next_layer: 251,
            current_binding: None,
            last_bank: Bank::empty(),
            shader_tx: None,
            detour_tx: None,
        }
    }

    pub fn set_shader_channel(&mut self, tx: Sender<ShaderCommand>) {
        self.shader_tx = Some(tx);
    }

    pub fn set_detour_channel(&mut self, tx: Sender<DetourCommand>) {
        self.detour_tx = Some(tx);
    }

    fn alloc_layer(&mut self) -> u32 {
        let l = self.next_layer;
        self.next_layer = if l == 0 { 254 } else { l - 1 };
        l
    }

    /// Bind a slot to `next` and start the load.
    pub fn queue_next(&mut self, slot: Slot) -> Result<()> {
        let layer = self.alloc_layer();
        self.next.layer = layer;
        self.next.try_load(slot)
    }

    /// Bind a slot to `current` and start playing immediately. Skips the
    /// rotation entirely. Used by the `trigger_slot` action.
    pub fn jump_to(&mut self, slot: Slot) -> Result<()> {
        let layer = self.alloc_layer();
        self.current.layer = layer;
        self.current.try_load(slot)?;
        Ok(())
    }

    /// Pump player buses and advance the rack FSM. Call once per main-loop tick.
    pub fn tick(&mut self) {
        self.last.tick();
        self.current.tick();
        self.next.tick();

        match self.settings.loop_type {
            LoopType::Sequential => self.tick_sequential(),
            LoopType::Parallel => self.tick_parallel(),
        }
    }

    fn tick_sequential(&mut self) {
        if self.current.status == PlayerStatus::Loaded {
            self.current.play();
        }
        // While current is playing and next is empty, pre-queue the successor
        // slot so that OnFinish::Switch can swap to a pre-rolled player.
        if self.current.status == PlayerStatus::Playing && self.next.status == PlayerStatus::Empty {
            if let Some((_b, s)) = self.current_binding {
                // Clone rng so we don't advance the main rng state for this speculative peek.
                let mut rng_clone = self.rng.clone();
                if let Some(next_slot) = crate::sample::context::get_next_context(
                    &self.last_bank,
                    s,
                    &self.settings,
                    &mut rng_clone,
                ) {
                    if let Err(e) = self.queue_next(next_slot) {
                        warn!("pre-queue next slot failed: {e}");
                    }
                }
            }
        }
        if self.current.status == PlayerStatus::Finished {
            match self.settings.on_finish {
                OnFinish::Switch => {
                    if self.next.status == PlayerStatus::Loaded {
                        self.swap();
                        self.current.play();
                    }
                }
                OnFinish::Repeat => {
                    if let Some(slot) = self.current.slot.clone() {
                        if let Err(e) = self.current.try_load(slot) {
                            warn!("repeat reload failed: {e}");
                        }
                    }
                }
            }
        }
    }

    fn tick_parallel(&mut self) {
        for p in [&mut self.current, &mut self.next] {
            if p.status == PlayerStatus::Loaded {
                p.play();
            }
            if p.status == PlayerStatus::Finished {
                if let Some(slot) = p.slot.clone() {
                    let _ = p.try_load(slot);
                }
            }
        }
    }

    fn swap(&mut self) {
        // last <- current <- next <- last (rotate)
        std::mem::swap(&mut self.last, &mut self.current);
        std::mem::swap(&mut self.current, &mut self.next);
        self.last.unload();
        // Phase-1 simplification: clear binding on swap so loop-point edits
        // only affect a player that was explicitly triggered, not auto-advanced.
        // Future: track next_binding and promote it here.
        self.current_binding = None;
    }

    pub fn now_mut(&mut self) -> &mut Player {
        &mut self.current
    }

    pub fn drain_last_error(&mut self) -> Option<String> {
        self.last.last_error.take()
            .or_else(|| self.current.last_error.take())
            .or_else(|| self.next.last_error.take())
    }
}

impl RackHandle for PlayerRack {
    fn reload_all(&mut self) {
        self.last.unload();
        self.current.unload();
        self.next.unload();
    }
    fn trigger_slot_with(&mut self, bank: u8, slot_idx: u8, slot: Slot, bank_snapshot: Bank) {
        self.current_binding = Some((bank, slot_idx));
        self.last_bank = bank_snapshot;
        // Clear next so tick_sequential will pre-queue the fresh successor.
        self.next.unload();
        if let Err(e) = self.jump_to(slot) {
            warn!("trigger_slot_with {bank}-{slot_idx} failed: {e}");
        }
    }
    fn current_position(&self) -> Option<f64> {
        if self.current.slot.is_some() {
            Some(self.current.last_position)
        } else {
            None
        }
    }
    fn current_binding(&self) -> Option<(u8, u8)> {
        self.current_binding
    }
    fn toggle_play_pause_now(&mut self) {
        match self.current.status {
            PlayerStatus::Playing => self.current.pause(),
            _ => self.current.play(),
        }
    }
    fn seek_relative_now(&mut self, _seconds: f64) {
        // gst seek_simple wrapper — implementation deferred to a focused task
        // when the UI hooks it up. For Phase-1 done-criteria the apply trait
        // accepts the action; the rack just no-ops for now.
    }
    fn set_rate_now(&mut self, _rate: f32) {
        // ditto: deferred.
    }
    fn trigger_shader(&mut self, name: &str, params: [f32; 8]) {
        if let Some(tx) = &self.shader_tx {
            let _ = tx.send(ShaderCommand::Trigger(name.to_string(), params));
        }
    }
    fn clear_shader(&mut self) {
        if let Some(tx) = &self.shader_tx {
            let _ = tx.send(ShaderCommand::Clear);
        }
    }
    fn set_shader_params(&mut self, params: [f32; 8]) {
        if let Some(tx) = &self.shader_tx {
            let _ = tx.send(ShaderCommand::SetParams(params));
        }
    }
    fn detour_scrub_by(&mut self, delta: i32) {
        if let Some(tx) = &self.detour_tx {
            let _ = tx.send(DetourCommand::ScrubBy(delta));
        }
    }

    fn start_recording(
        &mut self,
        _device_path: &str,
        _file_path: &std::path::Path,
        _target: crate::capture::recording::Target,
    ) -> crate::error::Result<()> {
        // Real implementation lands in Task 11 (gst-side hot-swap).
        Ok(())
    }

    fn stop_recording(&mut self) {
        // Real implementation in Task 11.
    }

    fn drain_finalized(&mut self) -> Vec<std::path::PathBuf> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{Slot, SourceKind};

    #[test]
    fn trigger_shader_sends_command_when_channel_set() {
        let mut r = PlayerRack::new(SamplerSettings::default(), 720, 480);
        let (tx, rx) = crossbeam_channel::unbounded();
        r.set_shader_channel(tx);
        use crate::apply::RackHandle;
        r.trigger_shader("color_shift", [0.5; 8]);
        r.clear_shader();
        assert!(matches!(rx.try_recv(), Ok(ShaderCommand::Trigger(name, _)) if name == "color_shift"));
        assert!(matches!(rx.try_recv(), Ok(ShaderCommand::Clear)));
    }

    #[test]
    fn trigger_shader_without_channel_is_silent_noop() {
        let mut r = PlayerRack::new(SamplerSettings::default(), 720, 480);
        use crate::apply::RackHandle;
        r.trigger_shader("color_shift", [0.0; 8]);
        r.clear_shader();
        // No panic = pass.
    }

    #[test]
    fn alloc_layer_wraps_at_zero() {
        let mut r = PlayerRack::new(SamplerSettings::default(), 720, 480);
        r.next_layer = 0;
        assert_eq!(r.alloc_layer(), 0);
        assert_eq!(r.next_layer, 254);
    }

    #[test]
    fn trigger_stores_bank_snapshot() {
        let mut r = PlayerRack::new(SamplerSettings::default(), 720, 480);
        let mut bank = Bank::empty();
        bank.slots[0] = Some(Slot {
            source: SourceKind::File("/tmp/x.mp4".into()),
            name: "x".into(),
            start: -1.0,
            end: -1.0,
            length: 0.0,
            rate: 1.0,
        });
        use crate::apply::RackHandle;
        let slot = bank.slots[0].clone().unwrap();
        r.trigger_slot_with(0, 0, slot, bank.clone());
        assert!(r.last_bank.slots[0].is_some());
        assert_eq!(r.current_binding, Some((0, 0)));
    }

    #[test]
    fn reload_all_clears_all_three() {
        let mut r = PlayerRack::new(SamplerSettings::default(), 720, 480);
        r.last.status = PlayerStatus::Loaded;
        r.current.status = PlayerStatus::Playing;
        r.next.status = PlayerStatus::Loaded;
        r.reload_all();
        assert_eq!(r.last.status, PlayerStatus::Empty);
        assert_eq!(r.current.status, PlayerStatus::Empty);
        assert_eq!(r.next.status, PlayerStatus::Empty);
    }

    #[test]
    fn drain_last_error_pulls_from_any_player() {
        let mut r = PlayerRack::new(SamplerSettings::default(), 720, 480);
        r.current.last_error = Some("decode failed".into());
        assert_eq!(r.drain_last_error(), Some("decode failed".into()));
        assert!(r.drain_last_error().is_none());
    }

    #[test]
    fn detour_scrub_sends_command_when_channel_set() {
        let mut r = PlayerRack::new(SamplerSettings::default(), 720, 480);
        let (tx, rx) = crossbeam_channel::unbounded();
        r.set_detour_channel(tx);
        use crate::apply::RackHandle;
        r.detour_scrub_by(-5);
        assert!(matches!(rx.try_recv(), Ok(DetourCommand::ScrubBy(-5))));
    }
}
