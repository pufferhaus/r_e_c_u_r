//! Live state for the DetourScrub mode: read position, speed, direction, mix,
//! markers, auto-play.

const SPEED_CYCLE: &[f32] = &[1.0, 2.0, 4.0, 0.25, 0.5];
const MIX_CYCLE: &[f32] = &[0.0, 0.25, 0.5, 0.75, 1.0];

#[derive(Debug, Clone, PartialEq)]
pub struct DetourSettings {
    pub speed: f32,
    pub forward: bool,
    pub mix: f32,
    pub start_marker: Option<usize>,
    pub end_marker: Option<usize>,
    pub read_position: usize,
    pub auto_play: bool,
    pub read_accumulator: f32,
}

impl Default for DetourSettings {
    fn default() -> Self {
        Self {
            speed: 1.0,
            forward: true,
            mix: 0.5,
            start_marker: None,
            end_marker: None,
            read_position: 0,
            auto_play: false,
            read_accumulator: 0.0,
        }
    }
}

impl DetourSettings {
    pub fn cycle_speed(&mut self) {
        let i = SPEED_CYCLE.iter().position(|&s| (s - self.speed).abs() < 1e-6).unwrap_or(0);
        self.speed = SPEED_CYCLE[(i + 1) % SPEED_CYCLE.len()];
    }

    pub fn cycle_mix(&mut self) {
        let i = MIX_CYCLE.iter().position(|&m| (m - self.mix).abs() < 1e-6).unwrap_or(0);
        self.mix = MIX_CYCLE[(i + 1) % MIX_CYCLE.len()];
    }

    pub fn scrub_by(&mut self, delta: i32, count: usize) {
        if count == 0 {
            self.read_position = 0;
            return;
        }
        let max = count - 1;
        let pos = self.read_position as i64 + delta as i64;
        self.read_position = pos.max(0).min(max as i64) as usize;
    }

    pub fn set_start_marker(&mut self) {
        self.start_marker = Some(self.read_position);
    }

    pub fn set_end_marker(&mut self) {
        self.end_marker = Some(self.read_position);
    }

    pub fn clear_markers(&mut self) {
        self.start_marker = None;
        self.end_marker = None;
    }

    pub fn toggle_direction(&mut self) {
        self.forward = !self.forward;
    }

    pub fn toggle_play(&mut self) {
        self.auto_play = !self.auto_play;
        self.read_accumulator = 0.0;
    }

    pub fn tick_auto_play(&mut self, count: usize) {
        if !self.auto_play || count == 0 {
            return;
        }
        self.read_accumulator += self.speed;
        let steps = self.read_accumulator.floor() as i64;
        if steps == 0 {
            return;
        }
        self.read_accumulator -= steps as f32;
        let signed = if self.forward { steps } else { -steps };
        let (lo, hi) = match (self.start_marker, self.end_marker) {
            (Some(a), Some(b)) if a < b => (a, b.min(count - 1)),
            _ => (0, count - 1),
        };
        let span = (hi - lo + 1) as i64;
        let from_lo = (self.read_position as i64 - lo as i64).rem_euclid(span);
        let advanced = (from_lo + signed).rem_euclid(span);
        self.read_position = (lo as i64 + advanced) as usize;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_settings_are_50_percent_mix_forward_play_off() {
        let d = DetourSettings::default();
        assert_eq!(d.speed, 1.0);
        assert!(d.forward);
        assert!((d.mix - 0.5).abs() < 1e-6);
        assert!(d.start_marker.is_none());
        assert!(d.end_marker.is_none());
        assert!(!d.auto_play);
    }

    #[test]
    fn cycle_speed_visits_each_step_in_order() {
        let mut d = DetourSettings::default();
        let order = [1.0, 2.0, 4.0, 0.25, 0.5, 1.0];
        d.speed = 1.0;
        for expected_next in order.iter().skip(1) {
            d.cycle_speed();
            assert!((d.speed - expected_next).abs() < 1e-6, "{} vs {}", d.speed, expected_next);
        }
    }

    #[test]
    fn cycle_mix_visits_each_step_in_order() {
        let mut d = DetourSettings::default();
        d.mix = 0.0;
        let order = [0.0, 0.25, 0.5, 0.75, 1.0, 0.0];
        for expected_next in order.iter().skip(1) {
            d.cycle_mix();
            assert!((d.mix - expected_next).abs() < 1e-6);
        }
    }

    #[test]
    fn scrub_by_clamps_to_count() {
        let mut d = DetourSettings::default();
        d.read_position = 5;
        d.scrub_by(10, 8);
        assert_eq!(d.read_position, 7);
        d.scrub_by(-99, 8);
        assert_eq!(d.read_position, 0);
    }

    #[test]
    fn scrub_by_with_zero_count_clamps_to_zero() {
        let mut d = DetourSettings::default();
        d.scrub_by(5, 0);
        assert_eq!(d.read_position, 0);
    }

    #[test]
    fn set_start_marker_records_current_read_position() {
        let mut d = DetourSettings::default();
        d.read_position = 42;
        d.set_start_marker();
        assert_eq!(d.start_marker, Some(42));
    }

    #[test]
    fn clear_markers_resets_both() {
        let mut d = DetourSettings::default();
        d.start_marker = Some(5);
        d.end_marker = Some(10);
        d.clear_markers();
        assert!(d.start_marker.is_none());
        assert!(d.end_marker.is_none());
    }

    #[test]
    fn tick_auto_play_advances_by_speed_each_call() {
        let mut d = DetourSettings::default();
        d.auto_play = true;
        d.speed = 2.0;
        d.forward = true;
        d.tick_auto_play(100);
        assert_eq!(d.read_position, 2);
        d.tick_auto_play(100);
        assert_eq!(d.read_position, 4);
    }

    #[test]
    fn tick_auto_play_fractional_speed_accumulates() {
        let mut d = DetourSettings::default();
        d.auto_play = true;
        d.speed = 0.25;
        for _ in 0..3 {
            d.tick_auto_play(100);
        }
        assert_eq!(d.read_position, 0);
        d.tick_auto_play(100);
        assert_eq!(d.read_position, 1);
    }

    #[test]
    fn tick_auto_play_wraps_at_count() {
        let mut d = DetourSettings::default();
        d.auto_play = true;
        d.speed = 1.0;
        d.read_position = 9;
        d.tick_auto_play(10);
        assert_eq!(d.read_position, 0);
    }
}
