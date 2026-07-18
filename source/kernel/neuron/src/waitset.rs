// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! RFC-0033: Waitset kernel object — wait on multiple endpoints, wake on first ready.
//!
//! CONTEXT: A waitset is a bounded set of endpoint members. A task blocks on the
//! whole set via `waitset_wait`; it wakes as soon as *any* member has a pending
//! message (or the deadline elapses). Because kernel timers "fire" by sending to a
//! notify endpoint (see `timer.rs` + `process_expired_timers`), a timer-notify
//! endpoint can be a waitset member — so one waitset unifies command, completion,
//! and *deterministic pacing* waits without a recv-timeout-as-clock hack.
//!
//! OWNERS: @kernel
//! STATUS: Draft (RFC-0033 Phase 1)
//! API_STABILITY: Unstable
//! TEST_COVERAGE: host unit tests (this file) + QEMU selftest marker `SELFTEST: waitset wake ok`
//!
//! DESIGN: This module holds only the *pure table logic* (membership + bookkeeping)
//! and a pure readiness-aggregation helper [`first_ready`]. It is deliberately free
//! of any riscv/MMIO/router coupling (endpoints and owners are raw `u32` ids, exactly
//! as `CapabilityKind::Timer` stores a raw `u32`), so it compiles and is unit-tested
//! on the host. The syscall layer (`syscall/api.rs`, riscv-only) maps `EndpointId`/`Pid`
//! to these ids and drives the block/wake machinery; that integration is proven at
//! boot by the QEMU selftest. Keeping the data structure host-testable makes the host
//! the deterministic oracle (RFC-0033 §Proof) — QEMU timing never decides correctness.
//!
//! INVARIANTS:
//! - Bounded table: at most `MAX_WAITSETS` live waitsets; over-limit → `ResourceExhausted`.
//! - Bounded members: at most `MAX_WAITSET_MEMBERS` (32) per waitset; over-limit → `ResourceExhausted`.
//! - Member set is deduplicated: adding an endpoint already present is a no-op (`Ok`).
//! - Ownership: a waitset records its owner pid; the syscall layer rejects cross-owner use.
//! - Cap lifecycle: closing a waitset cap frees its table entry (no dangling members).

// On the host, the only consumer of this table is the unit-test module below — the
// syscall layer that drives it is riscv-only (`cfg(target_os = "none")`). Suppress the
// resulting dead-code noise off-target; the riscv build keeps strict `deny(warnings)`
// dead-code detection because the syscall handlers there exercise every item.
#![cfg_attr(not(target_os = "none"), allow(dead_code))]

extern crate alloc;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::fmt;

/// Maximum number of concurrent waitsets (matches the timer table bound).
pub(crate) const MAX_WAITSETS: usize = 64;

/// Maximum number of endpoint members per waitset (RFC-0033 hard bound). 32 covers init's
/// full control-channel set (~23 services) so its routing responder can block reactively on
/// all of them at once instead of busy-polling.
pub(crate) const MAX_WAITSET_MEMBERS: usize = 32;

/// Opaque kernel-local waitset identifier (type-safe currency for the table API).
///
/// The capability stores the raw `u32` (mirroring `CapabilityKind::Timer(u32)`), so
/// `cap` stays independent of this module; the syscall layer wraps/unwraps the newtype.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct WaitsetId(pub u32);

/// Waitset operation error codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitsetError {
    /// No free waitset slots, or the member set is already full.
    ResourceExhausted,
    /// Waitset id not found in the table.
    InvalidHandle,
}

/// A single waitset: its owner and its bounded member set.
#[derive(Debug, Clone)]
pub struct WaitsetEntry {
    /// Task that owns this waitset (lifecycle + cross-owner rejection).
    pub owner_pid: u32,
    /// Endpoint ids that make up the set (deduplicated, `<= MAX_WAITSET_MEMBERS`).
    pub members: Vec<u32>,
}

/// Per-hart waitset table.
pub struct WaitsetTable {
    /// waitset_id -> entry.
    table: BTreeMap<u32, WaitsetEntry>,
    /// Next available waitset id.
    next_id: u32,
}

impl WaitsetTable {
    pub const fn new() -> Self {
        Self { table: BTreeMap::new(), next_id: 1 }
    }

    /// Allocate a new, empty waitset owned by `owner_pid`. Returns its id.
    pub fn alloc(&mut self, owner_pid: u32) -> Result<WaitsetId, WaitsetError> {
        if self.table.len() >= MAX_WAITSETS {
            return Err(WaitsetError::ResourceExhausted);
        }
        // Wrap-around collision handling: probe until we find a free id (mirror HartTimers).
        let mut id = self.next_id;
        loop {
            self.next_id = self.next_id.wrapping_add(1);
            if self.next_id == 0 {
                self.next_id = 1;
            }
            if !self.table.contains_key(&id) {
                break;
            }
            id = self.next_id;
        }
        self.table.insert(id, WaitsetEntry { owner_pid, members: Vec::new() });
        Ok(WaitsetId(id))
    }

    /// Add `endpoint` as a member. Bounded + deduplicated.
    ///
    /// Returns `Ok` if the endpoint is now a member (including the already-present case);
    /// `ResourceExhausted` if the set is full; `InvalidHandle` if the waitset is unknown.
    pub fn add_member(&mut self, id: WaitsetId, endpoint: u32) -> Result<(), WaitsetError> {
        let entry = self.table.get_mut(&id.0).ok_or(WaitsetError::InvalidHandle)?;
        if entry.members.iter().any(|e| *e == endpoint) {
            return Ok(());
        }
        if entry.members.len() >= MAX_WAITSET_MEMBERS {
            return Err(WaitsetError::ResourceExhausted);
        }
        entry.members.push(endpoint);
        Ok(())
    }

    /// Borrow the member endpoint ids of a waitset.
    pub fn members(&self, id: WaitsetId) -> Option<&[u32]> {
        self.table.get(&id.0).map(|e| e.members.as_slice())
    }

    /// Check whether a waitset exists and is owned by `pid`.
    pub fn owned_by(&self, id: WaitsetId, pid: u32) -> bool {
        self.table.get(&id.0).is_some_and(|e| e.owner_pid == pid)
    }

    /// Remove a waitset entirely (called on cap close). No dangling members.
    pub fn free(&mut self, id: WaitsetId) -> Result<(), WaitsetError> {
        self.table.remove(&id.0).ok_or(WaitsetError::InvalidHandle)?;
        Ok(())
    }

    #[cfg(test)]
    /// Number of live waitsets (test helper).
    pub fn len(&self) -> usize {
        self.table.len()
    }
}

impl fmt::Debug for WaitsetTable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WaitsetTable")
            .field("table_len", &self.table.len())
            .field("next_id", &self.next_id)
            .finish()
    }
}

/// Pure readiness-aggregation: index of the first member for which `is_ready` holds.
///
/// This is the level-ready scan at the heart of `waitset_wait` (RFC-0033 §1). It is a
/// free function so the *aggregation policy* (first-ready, in member order) is unit-tested
/// on the host independently of the router. The syscall passes a closure that queries
/// `router.pending(endpoint)`; here it is any predicate, keeping the logic deterministic.
#[inline]
pub fn first_ready(members: &[u32], is_ready: impl Fn(u32) -> bool) -> Option<usize> {
    members.iter().position(|&ep| is_ready(ep))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new_table() -> WaitsetTable {
        WaitsetTable::new()
    }

    #[test]
    fn alloc_increments_id_and_tracks_owner() {
        let mut t = new_table();
        let a = t.alloc(7).unwrap();
        let b = t.alloc(7).unwrap();
        assert_ne!(a, b);
        assert!(t.owned_by(a, 7));
        assert!(t.owned_by(b, 7));
        assert!(!t.owned_by(a, 9));
        assert_eq!(t.len(), 2);
    }

    #[test]
    fn add_member_is_bounded_at_16() {
        let mut t = new_table();
        let ws = t.alloc(1).unwrap();
        for ep in 0..MAX_WAITSET_MEMBERS as u32 {
            t.add_member(ws, 100 + ep).unwrap();
        }
        assert_eq!(t.members(ws).unwrap().len(), MAX_WAITSET_MEMBERS);
        // The 17th distinct member is rejected, deterministically, with no partial state.
        assert_eq!(t.add_member(ws, 999), Err(WaitsetError::ResourceExhausted));
        assert_eq!(t.members(ws).unwrap().len(), MAX_WAITSET_MEMBERS);
    }

    #[test]
    fn add_member_dedups() {
        let mut t = new_table();
        let ws = t.alloc(1).unwrap();
        t.add_member(ws, 42).unwrap();
        t.add_member(ws, 42).unwrap(); // no-op, still Ok
        t.add_member(ws, 43).unwrap();
        assert_eq!(t.members(ws).unwrap(), &[42, 43]);
    }

    #[test]
    fn add_member_to_unknown_waitset_rejected() {
        let mut t = new_table();
        assert_eq!(t.add_member(WaitsetId(123), 1), Err(WaitsetError::InvalidHandle));
    }

    #[test]
    fn table_is_bounded() {
        let mut t = new_table();
        for _ in 0..MAX_WAITSETS {
            t.alloc(1).unwrap();
        }
        assert_eq!(t.len(), MAX_WAITSETS);
        assert_eq!(t.alloc(1), Err(WaitsetError::ResourceExhausted));
    }

    #[test]
    fn free_removes_entry_and_is_idempotent_via_error() {
        let mut t = new_table();
        let ws = t.alloc(1).unwrap();
        t.free(ws).unwrap();
        assert!(t.members(ws).is_none());
        assert!(!t.owned_by(ws, 1));
        // Double free reports InvalidHandle (no panic, no dangling state).
        assert_eq!(t.free(ws), Err(WaitsetError::InvalidHandle));
    }

    #[test]
    fn freeing_one_does_not_disturb_others() {
        let mut t = new_table();
        let a = t.alloc(1).unwrap();
        let b = t.alloc(2).unwrap();
        t.add_member(b, 5).unwrap();
        t.free(a).unwrap();
        assert!(t.members(a).is_none());
        assert_eq!(t.members(b).unwrap(), &[5]);
        assert!(t.owned_by(b, 2));
    }

    #[test]
    fn first_ready_returns_first_in_member_order() {
        let members = [10u32, 20, 30];
        // none ready
        assert_eq!(first_ready(&members, |_| false), None);
        // only the last
        assert_eq!(first_ready(&members, |ep| ep == 30), Some(2));
        // first wins when several are ready (deterministic order, not arrival)
        assert_eq!(first_ready(&members, |ep| ep == 20 || ep == 30), Some(1));
        // empty set is never ready
        assert_eq!(first_ready(&[], |_| true), None);
    }
}
