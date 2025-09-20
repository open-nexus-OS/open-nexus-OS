//! Simple time-based animation helpers.

use crate::config::Easing;

/// Returns eased progress in [0..1] for given t in [0..1].
pub fn ease(easing: Easing, t: f32) -> f32 {
    match easing {
        Easing::Linear => t,
        Easing::CubicOut => {
            // cubic-bezier-ish "snappy" out curve
            let p = t - 1.0;
            p * p * p + 1.0
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub enum Direction {
    In,
    Out,
}

/// Basic 0..1 timeline with a target direction.
#[derive(Copy, Clone, Debug)]
pub struct Timeline {
    pub value: f32,         // current progress 0..1
    pub duration_ms: u32,
    pub easing: Easing,
    pub dir: Direction,
}

impl Timeline {
    pub fn new(duration_ms: u32, easing: Easing) -> Self {
        Self { value: 0.0, duration_ms, easing, dir: Direction::Out }
    }

    pub fn set_dir(&mut self, dir: Direction) {
        self.dir = dir;
    }

    pub fn set_immediate(&mut self, open: bool) {
        self.value = if open { 1.0 } else { 0.0 };
    }

    pub fn tick(&mut self, dt_ms: u32) {
        if self.duration_ms == 0 { self.value = if matches!(self.dir, Direction::In) {1.0} else {0.0}; return; }

        let delta = (dt_ms as f32) / (self.duration_ms as f32);
        let raw = if matches!(self.dir, Direction::In) {
            (self.value + delta).min(1.0)
        } else {
            (self.value - delta).max(0.0)
        };
        // Apply easing on a normalized time axis
        self.value = ease(self.easing, raw);
    }
}
