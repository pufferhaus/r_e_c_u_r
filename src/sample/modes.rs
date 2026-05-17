//! Sampler-mode math. Pure functions, deterministic given a `Rng`.

use rand::Rng;

use crate::state::{SamplerSettings, Slot};

/// Compute effective end time for `slot` under the current settings.
/// `fixed_length_mode` clamps end to `start + fixed_length * fixed_length_multiply`.
pub fn effective_end(slot: &Slot, settings: &SamplerSettings) -> f64 {
    let start = if slot.start > 0.0 { slot.start } else { 0.0 };
    let user_end = if slot.end > 0.0 { slot.end } else { slot.length };
    if settings.fixed_length_mode && settings.fixed_length > 0.0 {
        let fl = settings.fixed_length * settings.fixed_length_multiply as f64;
        (start + fl).min(if slot.length > 0.0 { slot.length } else { f64::MAX })
    } else {
        user_end
    }
}

/// Replace `slot.start` with a uniformly random offset in `[0, max_start]`,
/// where `max_start = max(0, length - clip_length)`. Pure given `rng`.
pub fn apply_rand_start<R: Rng>(slot: &mut Slot, settings: &SamplerSettings, rng: &mut R) {
    if !settings.rand_start_mode || slot.length <= 0.0 {
        return;
    }
    let clip_len = if slot.end > 0.0 {
        slot.end - slot.start.max(0.0)
    } else {
        1.0
    };
    let max_start = (slot.length - clip_len).max(0.0);
    slot.start = rng.gen_range(0.0..=max_start);
    if slot.end > 0.0 {
        slot.end = slot.start + clip_len;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::SourceKind;
    use rand::SeedableRng;
    use rand_chacha::ChaCha8Rng;

    fn slot(start: f64, end: f64, length: f64) -> Slot {
        Slot {
            source: SourceKind::File("/tmp/x.mp4".into()),
            name: "x".into(),
            start,
            end,
            length,
            rate: 1.0,
        }
    }

    #[test]
    fn effective_end_uses_user_end_by_default() {
        let s = slot(1.0, 4.0, 10.0);
        let mut settings = SamplerSettings::default();
        settings.fixed_length_mode = false;
        assert_eq!(effective_end(&s, &settings), 4.0);
    }

    #[test]
    fn effective_end_falls_back_to_length_when_end_unset() {
        let s = slot(0.0, -1.0, 10.0);
        let settings = SamplerSettings::default();
        assert_eq!(effective_end(&s, &settings), 10.0);
    }

    #[test]
    fn effective_end_clamps_to_fixed_length() {
        let s = slot(2.0, 9.0, 20.0);
        let mut settings = SamplerSettings::default();
        settings.fixed_length_mode = true;
        settings.fixed_length = 3.0;
        settings.fixed_length_multiply = 1.0;
        assert_eq!(effective_end(&s, &settings), 5.0);
    }

    #[test]
    fn rand_start_noop_when_mode_off() {
        let mut s = slot(0.5, 2.0, 10.0);
        let settings = SamplerSettings::default();
        let mut rng = ChaCha8Rng::seed_from_u64(1);
        apply_rand_start(&mut s, &settings, &mut rng);
        assert_eq!(s.start, 0.5);
    }

    #[test]
    fn rand_start_picks_inside_range() {
        let mut s = slot(0.0, 2.0, 10.0);
        let mut settings = SamplerSettings::default();
        settings.rand_start_mode = true;
        let mut rng = ChaCha8Rng::seed_from_u64(42);
        apply_rand_start(&mut s, &settings, &mut rng);
        assert!(s.start >= 0.0 && s.start <= 8.0, "got {}", s.start);
        assert!((s.end - s.start - 2.0).abs() < 1e-9);
    }
}
