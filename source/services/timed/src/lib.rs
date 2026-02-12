// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: timed daemon — deterministic timer coalescing authority
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Unit tests in this crate + QEMU marker proof via selftest-client
//! ADR: docs/adr/0017-service-architecture.md
//!
//! SECURITY INVARIANTS:
//! - Timer registrations are bounded per caller (max 64 live timers)
//! - Coalescing windows are deterministic and QoS-class based
//! - Invalid QoS and malformed requests are rejected deterministically

#![forbid(unsafe_code)]
#![cfg_attr(
    all(feature = "os-lite", nexus_env = "os", target_arch = "riscv64", target_os = "none"),
    no_std
)]

extern crate alloc;

use alloc::vec::Vec;

#[cfg(all(feature = "os-lite", nexus_env = "os"))]
mod os_lite;

#[cfg(all(feature = "os-lite", nexus_env = "os"))]
pub use os_lite::*;

/// Timer protocol constants shared by client and service.
pub mod protocol {
    pub const MAGIC0: u8 = b'T';
    pub const MAGIC1: u8 = b'M';
    pub const VERSION: u8 = 1;

    pub const OP_REGISTER: u8 = 1;
    pub const OP_CANCEL: u8 = 2;
    pub const OP_SLEEP_UNTIL: u8 = 3;
    pub const OP_RESPONSE: u8 = 0x80;

    pub const STATUS_OK: u8 = 0;
    pub const STATUS_INVALID_ARGS: u8 = 1;
    pub const STATUS_OVER_LIMIT: u8 = 2;
    pub const STATUS_NOT_FOUND: u8 = 3;
    pub const STATUS_MALFORMED: u8 = 4;
    pub const STATUS_INTERNAL: u8 = 5;

    pub const MIN_FRAME_LEN: usize = 4;
    pub const REGISTER_REQ_LEN: usize = 18;
    pub const CANCEL_REQ_LEN: usize = 12;
    pub const SLEEP_REQ_LEN: usize = 18;
}

/// Hard per-owner timer registration cap.
pub const MAX_TIMERS_PER_OWNER: usize = 64;
/// Global cap to keep service memory bounded under multi-tenant load.
pub const MAX_TIMERS_TOTAL: usize = 1024;
/// Maximum blocking sleep window accepted by OP_SLEEP_UNTIL.
pub const MAX_SLEEP_NS: u64 = 2_000_000_000;

/// Deterministic coalescing windows by QoS class.
pub const QOS_WINDOW_IDLE_NS: u64 = 8_000_000;
pub const QOS_WINDOW_NORMAL_NS: u64 = 4_000_000;
pub const QOS_WINDOW_INTERACTIVE_NS: u64 = 1_000_000;
pub const QOS_WINDOW_PERF_BURST_NS: u64 = 0;

/// Returns deterministic coalescing window for a wire QoS value.
pub const fn coalescing_window_ns(qos_raw: u8) -> Option<u64> {
    match qos_raw {
        0 => Some(QOS_WINDOW_IDLE_NS),
        1 => Some(QOS_WINDOW_NORMAL_NS),
        2 => Some(QOS_WINDOW_INTERACTIVE_NS),
        3 => Some(QOS_WINDOW_PERF_BURST_NS),
        _ => None,
    }
}

/// Aligns a deadline upward into its deterministic coalescing bucket.
pub fn coalesced_deadline(deadline_ns: u64, qos_raw: u8) -> Option<u64> {
    let window = coalescing_window_ns(qos_raw)?;
    if window == 0 {
        return Some(deadline_ns);
    }
    let rem = deadline_ns % window;
    if rem == 0 {
        Some(deadline_ns)
    } else {
        deadline_ns.checked_add(window - rem)
    }
}

/// One live timer registration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimerEntry {
    pub owner_service_id: u64,
    pub id: u32,
    pub deadline_ns: u64,
}

#[must_use = "timer register rejects must be handled"]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegisterReject {
    OverLimit,
    NoSpace,
}

/// Bounded in-memory timer registry.
pub struct TimerRegistry {
    entries: Vec<TimerEntry>,
    next_id: u32,
}

impl TimerRegistry {
    /// Creates an empty bounded timer registry.
    pub fn new() -> Self {
        Self { entries: Vec::new(), next_id: 1 }
    }

    /// Registers a timer for `owner_service_id`, enforcing per-owner and global bounds.
    pub fn register(
        &mut self,
        owner_service_id: u64,
        deadline_ns: u64,
    ) -> Result<u32, RegisterReject> {
        if self.count_for_owner(owner_service_id) >= MAX_TIMERS_PER_OWNER {
            return Err(RegisterReject::OverLimit);
        }
        if self.entries.len() >= MAX_TIMERS_TOTAL {
            return Err(RegisterReject::NoSpace);
        }
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        if self.next_id == 0 {
            self.next_id = 1;
        }
        self.entries.push(TimerEntry { owner_service_id, id, deadline_ns });
        Ok(id)
    }

    /// Cancels a timer owned by `owner_service_id`.
    pub fn cancel(&mut self, owner_service_id: u64, id: u32) -> bool {
        if let Some(pos) = self
            .entries
            .iter()
            .position(|entry| entry.owner_service_id == owner_service_id && entry.id == id)
        {
            self.entries.swap_remove(pos);
            true
        } else {
            false
        }
    }

    /// Returns number of live timers for the given owner.
    pub fn count_for_owner(&self, owner_service_id: u64) -> usize {
        self.entries.iter().filter(|entry| entry.owner_service_id == owner_service_id).count()
    }
}

impl Default for TimerRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coalescing_windows_match_contract() {
        assert_eq!(coalescing_window_ns(0), Some(QOS_WINDOW_IDLE_NS));
        assert_eq!(coalescing_window_ns(1), Some(QOS_WINDOW_NORMAL_NS));
        assert_eq!(coalescing_window_ns(2), Some(QOS_WINDOW_INTERACTIVE_NS));
        assert_eq!(coalescing_window_ns(3), Some(QOS_WINDOW_PERF_BURST_NS));
        assert_eq!(coalescing_window_ns(4), None);
    }

    #[test]
    fn coalesced_deadline_aligns_up() {
        let d = 10_000_001u64;
        // Normal = 4ms => 4_000_000ns buckets.
        assert_eq!(coalesced_deadline(d, 1), Some(12_000_000));
    }

    #[test]
    fn test_reject_timer_registration_over_limit() {
        let mut reg = TimerRegistry::new();
        let owner = 0xAABB_CCDD_EEFF_0011u64;
        for i in 0..MAX_TIMERS_PER_OWNER {
            let id = reg.register(owner, i as u64).expect("within per-owner cap");
            assert_ne!(id, 0);
        }
        let reject = reg.register(owner, 123).unwrap_err();
        assert_eq!(reject, RegisterReject::OverLimit);
    }

    #[test]
    fn test_reject_timer_registration_over_global_limit() {
        let mut reg = TimerRegistry::new();
        for i in 0..MAX_TIMERS_TOTAL {
            let owner = (i as u64) + 1;
            let id = reg.register(owner, i as u64).expect("within global cap");
            assert_ne!(id, 0);
        }
        let reject = reg.register(0xDEAD_BEEF, 1).unwrap_err();
        assert_eq!(reject, RegisterReject::NoSpace);
    }

    #[test]
    fn coalesced_deadline_rejects_invalid_qos_wire_value() {
        assert_eq!(coalesced_deadline(1_000_000, 255), None);
    }

    #[test]
    fn cancel_only_owner() {
        let mut reg = TimerRegistry::new();
        let owner_a = 1u64;
        let owner_b = 2u64;
        let id = reg.register(owner_a, 99).expect("register");
        assert!(!reg.cancel(owner_b, id));
        assert!(reg.cancel(owner_a, id));
    }
}
