// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! RFC-0033: Timeline fence kernel object — a monotonic `u64` value with target waits.
//!
//! CONTEXT: A fence holds a monotonically non-decreasing `value`. `fence_signal(v)`
//! advances it (`value = max(value, v)`); `fence_wait(target)` is satisfied as soon as
//! `value >= target`. This is the completion/ordering primitive for the submit ring
//! (RFC-0033 Phase 3): a producer signals the fence to a sequence number, consumers wait
//! for it. Signalling wakes every waiter whose target the new value now satisfies.
//!
//! OWNERS: @kernel
//! STATUS: Draft (RFC-0033 Phase 2)
//! API_STABILITY: Unstable
//! TEST_COVERAGE: host unit tests (this file) + QEMU selftest markers
//!   `KSELFTEST: fence wait ok` / `fence timeout ok`
//!
//! DESIGN: Like [`crate::waitset`], this module holds only the *pure table logic* and is
//! deliberately free of riscv/MMIO/router coupling (owners/pids are raw `u32`, the value
//! is a raw `u64`), so it compiles and is unit-tested on the host — the deterministic
//! oracle (RFC-0033 §Proof; see `docs/testing/README.md`). The syscall
//! layer (`syscall/api.rs`, riscv-only) maps caps/pids and drives the block/wake machinery
//! (`BlockReason::Fence` + `tasks.wake`); that integration is proven at boot by the QEMU
//! selftest. The waiter list is the only state the fence owns directly — it is **bounded**.
//!
//! LOCKING (TASK-0330 P2, ADR-0102): each fence sits behind its own lock (`sync::slot_table`,
//! class `ipc`); the table's methods take `&self`. The pids a signal satisfies are copied out
//! and woken without the fence's lock.
//!
//! INVARIANTS:
//! - Monotonic: `value` never decreases; `signal(v)` with `v <= value` is a no-op.
//! - An id whose fence was freed names nothing, also once its slot holds another fence.
//! - Bounded: at most `MAX_FENCES` live fences and `MAX_FENCE_WAITERS` waiters per fence;
//!   over-limit → `ResourceExhausted`, deterministic, no partial state.
//! - One waiter per pid: re-registering a pid replaces its target (no duplicate wakes).
//! - Cap lifecycle: closing a fence cap frees its table entry (no dangling waiters).

// On the host, the only consumer of this table is the unit-test module below — the syscall
// layer that drives it is riscv-only (`cfg(target_os = "none")`). Suppress the resulting
// dead-code noise off-target; the riscv build keeps strict `deny(warnings)` detection.
#![cfg_attr(not(target_os = "none"), allow(dead_code))]

extern crate alloc;
use alloc::vec::Vec;
use core::fmt;

use crate::sync::lock_rank::Class;
use crate::sync::slot_table::SlotTable;

/// Maximum number of concurrent fences (matches the timer/waitset table bound).
pub(crate) const MAX_FENCES: usize = 64;

/// Maximum number of waiters blocked on a single fence (RFC-0033 hard bound).
pub(crate) const MAX_FENCE_WAITERS: usize = 64;

/// Opaque kernel-local fence identifier (type-safe currency for the table API).
///
/// The capability stores the raw `u32` (mirroring `CapabilityKind::Timer(u32)`), so `cap`
/// stays independent of this module; the syscall layer wraps/unwraps the newtype.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct FenceId(pub u32);

/// Fence operation error codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FenceError {
    /// No free fence slots, or the waiter set is already full.
    ResourceExhausted,
    /// Fence id not found in the table.
    InvalidHandle,
}

/// A task blocked on a fence until `value >= target`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FenceWaiter {
    pid: u32,
    target: u64,
}

/// A single fence: its owner, current value, and bounded waiter set.
#[derive(Debug, Clone)]
struct FenceEntry {
    owner_pid: u32,
    value: u64,
    waiters: Vec<FenceWaiter>,
}

/// The fences: each behind its own lock (class `ipc`), found by its id without a table lock
/// (TASK-0330 P2, ADR-0102). An id whose fence was freed names nothing.
pub struct FenceTable {
    slots: SlotTable<FenceEntry>,
}

impl FenceTable {
    pub fn new() -> Self {
        Self { slots: SlotTable::new(Class::Ipc, MAX_FENCES) }
    }

    /// Allocate a new fence (value starts at 0) owned by `owner_pid`. Returns its id.
    pub fn alloc(&self, owner_pid: u32) -> Result<FenceId, FenceError> {
        self.slots
            .insert(FenceEntry { owner_pid, value: 0, waiters: Vec::new() })
            .map(FenceId)
            .ok_or(FenceError::ResourceExhausted)
    }

    #[cfg(test)]
    /// Current value of a fence (test helper; the kernel checks via `is_satisfied`, and a
    /// fence-value read syscall is out of scope for RFC-0033 Phase 2 — un-gate when a
    /// DriverKit consumer needs it).
    pub fn value(&self, id: FenceId) -> Option<u64> {
        self.slots.with(id.0, |e| e.value)
    }

    /// Check whether a fence exists (authority for signal/wait is cap
    /// possession; see `fence_id_from_cap`).
    pub fn exists(&self, id: FenceId) -> bool {
        self.slots.with(id.0, |_| ()).is_some()
    }

    /// Check whether a fence exists and is owned by `pid` (lifecycle only:
    /// free/teardown — NOT signal/wait authority, which travels with the cap).
    pub fn owned_by(&self, id: FenceId, pid: u32) -> bool {
        self.slots.with(id.0, |e| e.owner_pid == pid).unwrap_or(false)
    }

    /// Advance the fence monotonically to at least `v`. Returns the resulting value.
    ///
    /// `v <= value` is a no-op (the value never decreases). Waking satisfied waiters is the
    /// caller's job via [`take_satisfied`](Self::take_satisfied) (the kernel needs the pid
    /// list to call `tasks.wake`; keeping the two steps separate keeps both host-testable).
    pub fn signal(&self, id: FenceId, v: u64) -> Result<u64, FenceError> {
        let advance = |entry: &mut FenceEntry| {
            if v > entry.value {
                entry.value = v;
            }
            entry.value
        };
        self.slots.with(id.0, advance).ok_or(FenceError::InvalidHandle)
    }

    /// Is `value >= target` for this fence?
    pub fn is_satisfied(&self, id: FenceId, target: u64) -> Option<bool> {
        self.slots.with(id.0, |e| e.value >= target)
    }

    /// Register `pid` as a waiter for `target`. Bounded + one-per-pid (re-registering a pid
    /// updates its target rather than adding a duplicate).
    pub fn register_waiter(&self, id: FenceId, pid: u32, target: u64) -> Result<(), FenceError> {
        let register = |entry: &mut FenceEntry| {
            if let Some(w) = entry.waiters.iter_mut().find(|w| w.pid == pid) {
                w.target = target;
                return Ok(());
            }
            if entry.waiters.len() >= MAX_FENCE_WAITERS {
                return Err(FenceError::ResourceExhausted);
            }
            entry.waiters.push(FenceWaiter { pid, target });
            Ok(())
        };
        self.slots.with(id.0, register).ok_or(FenceError::InvalidHandle)?
    }

    /// Remove `pid` from a fence's waiter set, if present (cleanup on wake/timeout/close).
    pub fn remove_waiter(&self, id: FenceId, pid: u32) {
        let _ = self.slots.with(id.0, |entry| entry.waiters.retain(|w| w.pid != pid));
    }

    /// Collect and remove every waiter the current value now satisfies (`target <= value`),
    /// writing their pids into `out`. Returns the count (bounded by `out.len()`).
    pub fn take_satisfied(&self, id: FenceId, out: &mut [u32]) -> usize {
        let take = |entry: &mut FenceEntry| {
            let value = entry.value;
            let mut n = 0;
            // Drain satisfied waiters; keep the rest. `out` caps how many we report this
            // pass — any overflow stays registered and is picked up on the next signal.
            let mut i = 0;
            while i < entry.waiters.len() {
                if entry.waiters[i].target <= value && n < out.len() {
                    out[n] = entry.waiters[i].pid;
                    n += 1;
                    entry.waiters.remove(i);
                } else {
                    i += 1;
                }
            }
            n
        };
        self.slots.with(id.0, take).unwrap_or(0)
    }

    /// Remove a fence entirely (called on cap close). No dangling waiters.
    pub fn free(&self, id: FenceId) -> Result<(), FenceError> {
        self.slots.remove(id.0).map(|_| ()).ok_or(FenceError::InvalidHandle)
    }

    /// Frees every fence `pid` owns — its exit (TASK-0333 6a); the waiters of each are
    /// returned so the caller wakes them (they re-run their wait and find no fence).
    pub fn free_owned_by(&self, pid: u32) -> Vec<u32> {
        let mut waiters = Vec::new();
        self.slots.remove_if(
            |entry| entry.owner_pid == pid,
            |entry| waiters.extend(entry.waiters.iter().map(|w| w.pid)),
        );
        waiters
    }

    /// Number of live fences (the record).
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    #[cfg(test)]
    /// Number of waiters registered on a fence (test helper).
    pub fn waiter_count(&self, id: FenceId) -> usize {
        self.slots.with(id.0, |e| e.waiters.len()).unwrap_or(0)
    }
}

impl fmt::Debug for FenceTable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FenceTable").field("len", &self.len()).finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new_table() -> FenceTable {
        FenceTable::new()
    }

    #[test]
    fn alloc_starts_at_zero_and_tracks_owner() {
        let t = new_table();
        let a = t.alloc(7).unwrap();
        let b = t.alloc(7).unwrap();
        assert_ne!(a, b);
        assert_eq!(t.value(a), Some(0));
        assert!(t.owned_by(a, 7));
        assert!(!t.owned_by(a, 9));
        assert_eq!(t.len(), 2);
    }

    #[test]
    fn signal_is_monotonic() {
        let t = new_table();
        let f = t.alloc(1).unwrap();
        assert_eq!(t.signal(f, 10).unwrap(), 10);
        // A lower signal does not lower the value.
        assert_eq!(t.signal(f, 3).unwrap(), 10);
        assert_eq!(t.value(f), Some(10));
        // Equal/higher advances as expected.
        assert_eq!(t.signal(f, 10).unwrap(), 10);
        assert_eq!(t.signal(f, 11).unwrap(), 11);
    }

    #[test]
    fn is_satisfied_tracks_value() {
        let t = new_table();
        let f = t.alloc(1).unwrap();
        assert_eq!(t.is_satisfied(f, 0), Some(true)); // value 0 >= target 0
        assert_eq!(t.is_satisfied(f, 1), Some(false));
        t.signal(f, 5).unwrap();
        assert_eq!(t.is_satisfied(f, 5), Some(true));
        assert_eq!(t.is_satisfied(f, 6), Some(false));
        assert_eq!(t.is_satisfied(FenceId(999), 0), None);
    }

    #[test]
    fn register_waiter_is_bounded_and_one_per_pid() {
        let t = new_table();
        let f = t.alloc(1).unwrap();
        for pid in 0..MAX_FENCE_WAITERS as u32 {
            t.register_waiter(f, pid, 100).unwrap();
        }
        assert_eq!(t.waiter_count(f), MAX_FENCE_WAITERS);
        // A new distinct pid over the bound is rejected.
        assert_eq!(t.register_waiter(f, 9999, 1), Err(FenceError::ResourceExhausted));
        // Re-registering an existing pid updates its target, not the count.
        t.register_waiter(f, 0, 250).unwrap();
        assert_eq!(t.waiter_count(f), MAX_FENCE_WAITERS);
    }

    #[test]
    fn take_satisfied_selects_only_reached_targets() {
        let t = new_table();
        let f = t.alloc(1).unwrap();
        t.register_waiter(f, 10, 5).unwrap();
        t.register_waiter(f, 11, 20).unwrap();
        t.register_waiter(f, 12, 8).unwrap();
        t.signal(f, 8).unwrap(); // satisfies targets 5 and 8, not 20

        let mut out = [0u32; MAX_FENCE_WAITERS];
        let n = t.take_satisfied(f, &mut out);
        assert_eq!(n, 2);
        // pids 10 and 12 woken (order = registration order), pid 11 still waiting.
        assert!(out[..n].contains(&10));
        assert!(out[..n].contains(&12));
        assert_eq!(t.waiter_count(f), 1);

        // Advancing past 20 now releases pid 11.
        t.signal(f, 100).unwrap();
        let n2 = t.take_satisfied(f, &mut out);
        assert_eq!(n2, 1);
        assert_eq!(out[0], 11);
        assert_eq!(t.waiter_count(f), 0);
    }

    #[test]
    fn remove_waiter_cleans_up() {
        let t = new_table();
        let f = t.alloc(1).unwrap();
        t.register_waiter(f, 5, 10).unwrap();
        t.remove_waiter(f, 5);
        assert_eq!(t.waiter_count(f), 0);
        // Removing an absent pid is a no-op.
        t.remove_waiter(f, 5);
    }

    #[test]
    fn table_is_bounded() {
        let t = new_table();
        for _ in 0..MAX_FENCES {
            t.alloc(1).unwrap();
        }
        assert_eq!(t.len(), MAX_FENCES);
        assert_eq!(t.alloc(1), Err(FenceError::ResourceExhausted));
    }

    /// TASK-0330 P2: a freed fence's id names nothing once another fence has its slot — a
    /// capability that outlived its fence neither signals nor waits on the new tenant.
    #[test]
    fn test_reject_a_freed_fence_id_after_its_slot_is_reused() {
        let t = new_table();
        let old = t.alloc(1).unwrap();
        t.free(old).unwrap();
        for _ in 0..MAX_FENCES {
            t.alloc(2).unwrap();
        }
        assert!(!t.exists(old));
        assert_eq!(t.signal(old, 9), Err(FenceError::InvalidHandle));
        assert_eq!(t.register_waiter(old, 3, 1), Err(FenceError::InvalidHandle));
        assert_eq!(t.is_satisfied(old, 0), None);
    }

    #[test]
    fn free_removes_entry_and_reports_double_free() {
        let t = new_table();
        let f = t.alloc(1).unwrap();
        t.free(f).unwrap();
        assert_eq!(t.value(f), None);
        assert_eq!(t.free(f), Err(FenceError::InvalidHandle));
    }
}
