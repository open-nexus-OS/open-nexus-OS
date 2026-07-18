// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! QoS hints for submit pacing — `Frugal` / `Normal` / `Burst`.
//!
//! A QoS class maps to a **target in-flight depth** for a [`crate::SubmitRing`]: how many
//! submissions the producer keeps outstanding before it backpressures. `Frugal` keeps a
//! single submission in flight (lowest power / latency floor — one wake per item), `Burst`
//! fills the ring (max throughput), `Normal` balances. This mirrors the kernel `QosClass` /
//! `timed` windows (RFC-0023) on the userland submit side.

/// Submit-pacing QoS class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Qos {
    /// Keep a single submission in flight — lowest power, drains promptly.
    Frugal,
    /// Balance latency and throughput (about half the ring in flight).
    #[default]
    Normal,
    /// Fill the ring — maximum throughput / coalescing.
    Burst,
}

impl Qos {
    /// Target number of in-flight submissions for a ring of `capacity` slots. Always
    /// `1..=capacity` (never zero — that would stall — and never over the ring).
    pub fn target_depth(&self, capacity: usize) -> usize {
        let cap = capacity.max(1);
        match self {
            Qos::Frugal => 1,
            Qos::Normal => (cap / 2).max(1),
            Qos::Burst => cap,
        }
    }

    /// Stable lowercase label (tracing / markers).
    pub fn as_str(&self) -> &'static str {
        match self {
            Qos::Frugal => "frugal",
            Qos::Normal => "normal",
            Qos::Burst => "burst",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_normal() {
        assert_eq!(Qos::default(), Qos::Normal);
    }

    #[test]
    fn target_depth_is_bounded_1_to_capacity() {
        for cap in [1usize, 2, 4, 8, 16, 32] {
            for q in [Qos::Frugal, Qos::Normal, Qos::Burst] {
                let d = q.target_depth(cap);
                assert!(d >= 1, "{:?}@{} depth {} < 1", q, cap, d);
                assert!(d <= cap, "{:?}@{} depth {} > cap", q, cap, d);
            }
        }
    }

    #[test]
    fn ordering_frugal_le_normal_le_burst() {
        let cap = 8;
        assert_eq!(Qos::Frugal.target_depth(cap), 1);
        assert!(Qos::Frugal.target_depth(cap) <= Qos::Normal.target_depth(cap));
        assert!(Qos::Normal.target_depth(cap) <= Qos::Burst.target_depth(cap));
        assert_eq!(Qos::Burst.target_depth(cap), cap);
    }

    #[test]
    fn zero_capacity_never_yields_zero_depth() {
        assert_eq!(Qos::Normal.target_depth(0), 1);
        assert_eq!(Qos::Burst.target_depth(0), 1);
    }

    #[test]
    fn labels_are_stable() {
        assert_eq!(Qos::Frugal.as_str(), "frugal");
        assert_eq!(Qos::Normal.as_str(), "normal");
        assert_eq!(Qos::Burst.as_str(), "burst");
    }
}
