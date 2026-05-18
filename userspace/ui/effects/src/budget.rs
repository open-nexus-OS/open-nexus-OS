// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Per-frame effect pixel budget with deterministic degrade.
//! Caps total pixels affected by blur/shadow per frame. When budget is
//! exhausted, remaining effects are skipped (no partial renders).

/// Per-frame budget for effect pixels.
/// Once `remaining` is exhausted, subsequent effects are skipped.
#[derive(Debug, Clone, Copy)]
pub struct EffectBudget {
    pub remaining: u32,
    pub total: u32,
}

impl EffectBudget {
    /// Create a new budget with the given total pixel allowance per frame.
    pub const fn new(pixels_per_frame: u32) -> Self {
        Self { remaining: pixels_per_frame, total: pixels_per_frame }
    }

    /// Try to reserve `count` pixels. Returns true if reserved, false if budget exhausted.
    /// Budget is consumed on success.
    pub fn try_reserve(&mut self, count: u32) -> bool {
        if count <= self.remaining {
            self.remaining = self.remaining.saturating_sub(count);
            true
        } else {
            false
        }
    }

    /// Reset the budget for a new frame.
    pub fn reset(&mut self) {
        self.remaining = self.total;
    }

    /// Fraction of budget remaining (0.0–1.0), as (numerator, denominator).
    /// Used for deterministic degrade signaling.
    pub fn fraction(&self) -> (u32, u32) {
        (self.remaining, self.total)
    }
}

// Default: allow 64K pixels of effects per frame (≈ 256×256 area)
impl Default for EffectBudget {
    fn default() -> Self {
        Self::new(65536)
    }
}
