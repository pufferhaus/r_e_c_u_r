//! Three-pipeline rotation: `last / current / next`. Hides decode latency.

use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;
use tracing::warn;

use crate::apply::RackHandle;
use crate::error::Result;
use crate::state::{Bank, LoopType, OnFinish, SamplerSettings, Slot};
use crate::video::player::{Player, PlayerStatus};

pub struct PlayerRack {
    pub last: Player,
    pub current: Player,
    pub next: Player,
    pub bank: Bank,
    pub bank_number: u8,
    pub settings: SamplerSettings,
    pub rng: ChaCha8Rng,
    /// Monotonically decreasing layer counter. Wraps at 0 → 254.
    pub next_layer: u32,
}

impl PlayerRack {
    pub fn new(bank: Bank, settings: SamplerSettings) -> Self {
        Self {
            last: Player::empty(254),
            current: Player::empty(253),
            next: Player::empty(252),
            bank,
            bank_number: 0,
            settings,
            rng: ChaCha8Rng::seed_from_u64(0xC0FFEE),
            next_layer: 251,
        }
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
    }

    pub fn now_mut(&mut self) -> &mut Player {
        &mut self.current
    }
}

impl RackHandle for PlayerRack {
    fn reload_all(&mut self) {
        self.last.unload();
        self.current.unload();
        self.next.unload();
    }
    fn trigger_slot(&mut self, _bank: u8, slot_idx: u8) {
        if let Some(Some(s)) = self.bank.slots.get(slot_idx as usize).cloned() {
            if let Err(e) = self.jump_to(s) {
                warn!("trigger_slot {slot_idx} failed: {e}");
            }
        }
    }
    fn set_loop_in_now(&mut self) {
        let pos = self.current.last_position;
        if let Some(slot) = &mut self.current.slot {
            slot.start = pos;
        }
    }
    fn set_loop_out_now(&mut self) {
        let pos = self.current.last_position;
        if let Some(slot) = &mut self.current.slot {
            slot.end = pos;
        }
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alloc_layer_wraps_at_zero() {
        let mut r = PlayerRack::new(Bank::empty(), SamplerSettings::default());
        r.next_layer = 0;
        assert_eq!(r.alloc_layer(), 0);
        assert_eq!(r.next_layer, 254);
    }

    #[test]
    fn reload_all_clears_all_three() {
        let mut r = PlayerRack::new(Bank::empty(), SamplerSettings::default());
        r.last.status = PlayerStatus::Loaded;
        r.current.status = PlayerStatus::Playing;
        r.next.status = PlayerStatus::Loaded;
        r.reload_all();
        assert_eq!(r.last.status, PlayerStatus::Empty);
        assert_eq!(r.current.status, PlayerStatus::Empty);
        assert_eq!(r.next.status, PlayerStatus::Empty);
    }

    #[test]
    fn set_loop_in_writes_current_position_to_slot() {
        let mut r = PlayerRack::new(Bank::empty(), SamplerSettings::default());
        r.current.slot = Some(Slot {
            location: "/tmp/x.mp4".into(),
            name: "x".into(),
            start: -1.0,
            end: -1.0,
            length: 10.0,
            rate: 1.0,
        });
        r.current.last_position = 2.5;
        r.set_loop_in_now();
        assert_eq!(r.current.slot.as_ref().unwrap().start, 2.5);
    }

    #[test]
    fn trigger_slot_with_empty_bank_is_noop() {
        let mut r = PlayerRack::new(Bank::empty(), SamplerSettings::default());
        r.trigger_slot(0, 5); // no panic
        assert_eq!(r.current.status, PlayerStatus::Empty);
    }
}
