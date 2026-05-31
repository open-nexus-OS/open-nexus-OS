// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

use alloc::vec::Vec;

/// Easing functions for keyframe interpolation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Easing {
    /// Constant velocity from start to end.
    Linear,
    /// Slow start, fast end.
    EaseIn,
    /// Fast start, slow end.
    EaseOut,
    /// Slow start and end, fast middle.
    EaseInOut,
}

/// A keyframe track: interpolates between keyframes over a fixed duration.
///
/// Keyframes are (progress, value) pairs where progress is 0.0..1.0.
/// The track advances deterministically with explicit dt.
#[derive(Debug, Clone, PartialEq)]
pub struct KeyframeTrack {
    /// Sorted by progress (0.0..1.0). First = 0.0, last = 1.0.
    keyframes: Vec<(f32, f32)>,
    /// Total duration in nanoseconds.
    duration_ns: u64,
    /// Easing function applied to the progress.
    easing: Easing,
    /// Elapsed time in nanoseconds.
    elapsed: u64,
    /// Current interpolated value.
    current_value: f32,
    done: bool,
}

impl KeyframeTrack {
    pub fn new(keyframes: Vec<(f32, f32)>, duration_ns: u64, easing: Easing) -> Self {
        assert!(!keyframes.is_empty(), "keyframes must not be empty");
        assert!((keyframes[0].0 - 0.0).abs() < 0.001, "first keyframe must be at 0.0");
        assert!(
            (keyframes[keyframes.len() - 1].0 - 1.0).abs() < 0.001,
            "last keyframe must be at 1.0"
        );

        let current_value = keyframes[0].1;
        Self {
            keyframes,
            duration_ns: duration_ns.max(1),
            easing,
            elapsed: 0,
            current_value,
            done: false,
        }
    }

    /// Advance by dt_ns. Returns current interpolated value.
    pub fn step(&mut self, dt_ns: u64) -> f32 {
        if self.done {
            return self.current_value;
        }

        self.elapsed = self.elapsed.saturating_add(dt_ns);
        if self.elapsed >= self.duration_ns {
            self.elapsed = self.duration_ns;
            self.current_value = self.keyframes.last().unwrap().1;
            self.done = true;
            return self.current_value;
        }

        let raw_progress = self.elapsed as f32 / self.duration_ns as f32;
        let progress = self.apply_easing(raw_progress);
        self.current_value = self.interpolate(progress);
        self.current_value
    }

    /// Returns true when the track has completed.
    pub fn done(&self) -> bool {
        self.done
    }

    pub fn value(&self) -> f32 {
        self.current_value
    }

    fn apply_easing(&self, t: f32) -> f32 {
        match self.easing {
            Easing::Linear => t,
            Easing::EaseIn => t * t,
            Easing::EaseOut => t * (2.0 - t),
            Easing::EaseInOut => {
                if t < 0.5 {
                    2.0 * t * t
                } else {
                    -1.0 + (4.0 - 2.0 * t) * t
                }
            }
        }
    }

    fn interpolate(&self, progress: f32) -> f32 {
        // Find surrounding keyframes
        let mut lower = &self.keyframes[0];
        let mut upper = &self.keyframes[self.keyframes.len() - 1];

        for window in self.keyframes.windows(2) {
            if progress >= window[0].0 && progress <= window[1].0 {
                lower = &window[0];
                upper = &window[1];
                break;
            }
        }

        let segment_progress = if (upper.0 - lower.0).abs() < 0.0001 {
            1.0
        } else {
            (progress - lower.0) / (upper.0 - lower.0)
        };

        lower.1 + (upper.1 - lower.1) * segment_progress
    }
}
