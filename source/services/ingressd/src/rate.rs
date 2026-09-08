// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Deterministic token bucket (RFC-0092 §3): `rate_per_s` tokens
//! per second up to `burst`, refilled from the caller's monotonic clock in
//! integer micro-tokens — no floats, no wall clock, saturating everywhere,
//! so the same timestamps always yield the same verdicts (the rate proof is
//! replayable on host and in QEMU).
//! OWNERS: @runtime @security
//! STATUS: Experimental
//! TEST_COVERAGE: unit tests below, tests/ingress_host/ (`test_reject_rate_exceeded`)

/// Micro-tokens per token.
const SCALE: u64 = 1_000_000;

/// One per open exposure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TokenBucket {
    rate_per_s: u32,
    burst: u32,
    /// Micro-tokens currently available.
    tokens: u64,
    last_ns: u64,
}

impl TokenBucket {
    /// A full bucket at `t = 0`.
    pub const fn new(rate_per_s: u32, burst: u32) -> Self {
        Self { rate_per_s, burst, tokens: burst as u64 * SCALE, last_ns: 0 }
    }

    /// Refills up to `now_ns` (a clock that went backwards refills nothing)
    /// and takes one token when available.
    pub fn try_take(&mut self, now_ns: u64) -> bool {
        if now_ns > self.last_ns {
            let elapsed = now_ns - self.last_ns;
            // tokens = elapsed_ns * rate / 1e9; micro-tokens = that * 1e6.
            let refill = elapsed.saturating_mul(u64::from(self.rate_per_s)) / 1_000;
            self.tokens = self.tokens.saturating_add(refill).min(u64::from(self.burst) * SCALE);
            self.last_ns = now_ns;
        }
        if self.tokens >= SCALE {
            self.tokens -= SCALE;
            true
        } else {
            false
        }
    }

    /// Whole tokens available right now (no refill).
    pub fn available(&self) -> u32 {
        u32::try_from(self.tokens / SCALE).unwrap_or(u32::MAX)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn burst_then_rate() {
        let mut b = TokenBucket::new(2, 3);
        assert!(b.try_take(0) && b.try_take(0) && b.try_take(0));
        assert!(!b.try_take(0));
        // 2/s → one token every 500 ms.
        assert!(!b.try_take(400_000_000));
        assert!(b.try_take(500_000_000));
        assert!(!b.try_take(500_000_000));
        // Refill is capped at burst.
        assert!(b.try_take(60_000_000_000) && b.try_take(60_000_000_000));
        assert!(b.try_take(60_000_000_000));
        assert!(!b.try_take(60_000_000_000));
    }

    #[test]
    fn clock_going_backwards_never_refills() {
        let mut b = TokenBucket::new(1000, 1);
        assert!(b.try_take(10_000_000_000));
        assert!(!b.try_take(1_000_000));
        assert!(!b.try_take(0));
        assert_eq!(b.available(), 0);
    }
}
