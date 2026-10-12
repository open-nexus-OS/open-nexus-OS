// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The one sync primitive (RFC-0101 §5), modelled: the kernel's timeline fence
//! (RFC-0033) is a monotonic `u64` a producer advances with `fence_signal` and a consumer
//! blocks on with `fence_wait`; this is the same object on the host, where there is no
//! kernel — a device model signals it, a compositor model waits on it, and a test reads it.
//! The point itself ([`TimelinePoint`]) is the driver-kit's: the device server and the client
//! name the same value.
//! OWNERS: @gpu @ui
//! STATUS: Functional
//! API_STABILITY: Unstable until RFC-0101 Phase 3 (which retires the boolean `core::fence`)
//! TEST_COVERAGE: unit tests (this file)
//! INVARIANTS: the value never decreases; `reached(p)` is true exactly when `value >= p`.

pub use nexus_driverkit::gpu::TimelinePoint;

/// The host model of one timeline fence.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Timeline {
    value: u64,
}

impl Timeline {
    /// A timeline at zero.
    pub const fn new() -> Self {
        Self { value: 0 }
    }

    /// The current value.
    pub const fn value(&self) -> TimelinePoint {
        TimelinePoint(self.value)
    }

    /// Advances to at least `point`; `true` if the value moved. A lower point is a no-op —
    /// the kernel's `fence_signal` rule, so no device can roll a timeline back.
    pub fn signal(&mut self, point: TimelinePoint) -> bool {
        if point.0 > self.value {
            self.value = point.0;
            true
        } else {
            false
        }
    }

    /// Whether a wait for `point` is satisfied.
    pub const fn reached(&self, point: TimelinePoint) -> bool {
        self.value >= point.0
    }

    /// The next point after the current value — what a producer signals for its next batch.
    pub const fn next(&self) -> TimelinePoint {
        TimelinePoint(self.value.saturating_add(1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wait_is_satisfied_when_the_value_reached_the_point() {
        let mut t = Timeline::new();
        let p = t.next();
        assert_eq!(p, TimelinePoint(1));
        assert!(!t.reached(p));
        assert!(t.signal(p));
        assert!(t.reached(p));
        assert!(t.reached(TimelinePoint(0)));
        assert!(!t.reached(t.next()));
    }

    #[test]
    fn test_reject_signal_backwards_is_a_no_op() {
        let mut t = Timeline::new();
        assert!(t.signal(TimelinePoint(5)));
        assert!(!t.signal(TimelinePoint(3)));
        assert!(!t.signal(TimelinePoint(5)));
        assert_eq!(t.value(), TimelinePoint(5));
        assert!(t.reached(TimelinePoint(3)), "an earlier point stays reached");
    }
}
