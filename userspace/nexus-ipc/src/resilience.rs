// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Typed circuit breaker for service recv loops (SMP robustness, TASK-0276 umbrella)
//! OWNERS: @services-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: host unit tests (streak reset, breach threshold, bounded logging)
//! PUBLIC API: CircuitBreaker::new(), on_success(), on_error(), BreakerVerdict
//! INVARIANTS: client-side IPC errors are transient and must never terminate a
//!   service; only a persistent CONSECUTIVE error streak (the service's own
//!   endpoint defect) may end the loop. The verdict is #[must_use] so the
//!   compiler rejects loops that silently ignore the continue/fatal decision —
//!   the exact bug class that killed logd under real parallelism.

/// Verdict of one error observation. `#[must_use]`: a serve loop that drops
/// this on the floor cannot compile-silently reintroduce die-on-error or
/// spin-forever-on-defect behavior.
#[must_use = "handle the verdict: Continue the loop or exit on EndpointDefect"]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BreakerVerdict {
    /// Transient: keep serving (yield first).
    Continue,
    /// The error streak exceeded the limit without a single successful recv:
    /// treat as a defect of the service's OWN endpoint and end the loop.
    EndpointDefect,
}

/// Bounded consecutive-error breaker for single-threaded server recv loops.
pub struct CircuitBreaker {
    consecutive: u32,
    limit: u32,
    logged: u32,
    log_limit: u32,
}

impl CircuitBreaker {
    /// `limit`: consecutive errors (without any success) that indicate an
    /// endpoint defect. `log_limit`: at most this many errors are reported
    /// via the `should_log` flag (bounded diagnostics, no log storms).
    pub const fn new(limit: u32, log_limit: u32) -> Self {
        Self { consecutive: 0, limit, logged: 0, log_limit }
    }

    /// A successful recv resets the streak.
    pub fn on_success(&mut self) {
        self.consecutive = 0;
    }

    /// Records an error. Returns `(should_log, verdict)`: `should_log` is
    /// true for the first `log_limit` errors of the boot (bounded), and the
    /// verdict decides whether the loop may continue.
    pub fn on_error(&mut self) -> (bool, BreakerVerdict) {
        self.consecutive = self.consecutive.saturating_add(1);
        let should_log = if self.logged < self.log_limit {
            self.logged += 1;
            true
        } else {
            false
        };
        let verdict = if self.consecutive >= self.limit {
            BreakerVerdict::EndpointDefect
        } else {
            BreakerVerdict::Continue
        };
        (should_log, verdict)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn success_resets_streak() {
        let mut b = CircuitBreaker::new(3, 3);
        let _ = b.on_error();
        let _ = b.on_error();
        b.on_success();
        let (_, v) = b.on_error();
        assert_eq!(v, BreakerVerdict::Continue);
    }

    #[test]
    fn breach_after_consecutive_limit() {
        let mut b = CircuitBreaker::new(3, 1);
        assert_eq!(b.on_error().1, BreakerVerdict::Continue);
        assert_eq!(b.on_error().1, BreakerVerdict::Continue);
        assert_eq!(b.on_error().1, BreakerVerdict::EndpointDefect);
    }

    #[test]
    fn logging_is_bounded_across_streaks() {
        let mut b = CircuitBreaker::new(100, 2);
        assert!(b.on_error().0);
        assert!(b.on_error().0);
        assert!(!b.on_error().0);
        b.on_success();
        // log budget is per boot, not per streak
        assert!(!b.on_error().0);
    }
}
