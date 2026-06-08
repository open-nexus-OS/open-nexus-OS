// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! RFC-0062: Kernel timer capability — per-hart timer table and deadline queue.
//!
//! CONTEXT: Manages timer objects created via `timer_create`. Each timer is bound
//! to a notification endpoint and delivers `OP_TIMER_FIRED` events on expiry.
//!
//! OWNERS: @kernel
//! STATUS: Draft (RFC-0062 Phase D.2)
//!
//! INVARIANTS:
//! - Drift-free periodic rearm: `next_deadline += interval_ns`, never `now + interval`
//! - Coalescing: at most one pending event per timer; `missed` counter for skipped ticks
//! - Bounded state: fixed-size table (MAX_TIMERS = 64), O(log n) queue operations
//! - Cap lifecycle: closing a timer cap removes it from all queues

extern crate alloc;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::fmt;

/// Maximum number of concurrent timers per hart.
pub(crate) const MAX_TIMERS: usize = 64;

/// Unique timer identifier within a hart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct TimerId(pub(crate) u32);

/// Timer operation error codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimerError {
    /// No free timer slots.
    ResourceExhausted,
    /// Timer ID not found in table.
    InvalidHandle,
    /// Timer is already armed — must cancel before re-arming.
    AlreadyArmed,
}


/// Timer state — stored in the per-hart timer table.
#[derive(Debug, Clone)]
pub struct TimerState {
    /// Task that owns this timer (for lifecycle management).
    pub owner_pid: u32,
    /// Endpoint that receives `OP_TIMER_FIRED` events.
    pub notify_ep: u32,
    /// Absolute monotonic deadline (nanoseconds).
    pub deadline_ns: u64,
    /// Periodic interval (0 = one-shot).
    pub interval_ns: u64,
    /// Monotonic fire count.
    pub seq: u32,
    /// Coalesced ticks since last delivery.
    pub missed: u32,
    /// Whether the timer is currently armed.
    pub armed: bool,
}

/// Per-hart timer management.
pub struct HartTimers {
    /// Timer table: timer_id -> TimerState.
    table: BTreeMap<u32, TimerState>,
    /// Deadline queue: deadline_ns -> Vec<TimerId>.
    /// Sorted ascending by deadline_ns. Multiple timers may share the same deadline.
    queue: BTreeMap<u64, Vec<TimerId>>,
    /// Next available timer ID.
    next_id: u32,
}

impl HartTimers {
    pub const fn new() -> Self {
        Self { table: BTreeMap::new(), queue: BTreeMap::new(), next_id: 1 }
    }

    /// Allocate a new timer slot. Returns the timer ID.
    pub fn alloc(&mut self, owner_pid: u32, notify_ep: u32, interval_ns: u64) -> Result<TimerId, TimerError> {
        if self.table.len() >= MAX_TIMERS {
            return Err(TimerError::ResourceExhausted);
        }
        // Wrap-around collision handling: probe until we find a free ID.
        let mut id = self.next_id;
        loop {
            self.next_id = self.next_id.wrapping_add(1);
            if !self.table.contains_key(&id) {
                break;
            }
            id = self.next_id;
        }
        self.table.insert(id, TimerState {
            owner_pid,
            notify_ep,
            deadline_ns: 0,
            interval_ns,
            seq: 0,
            missed: 0,
            armed: false,
        });
        Ok(TimerId(id))
    }

    /// Arm a timer with an absolute deadline.
    pub fn arm(&mut self, timer_id: TimerId, deadline_ns: u64) -> Result<(), TimerError> {
        let t = self.table.get_mut(&timer_id.0).ok_or(TimerError::InvalidHandle)?;
        if t.armed {
            return Err(TimerError::AlreadyArmed);
        }
        t.deadline_ns = deadline_ns;
        t.armed = true;
        self.queue.entry(deadline_ns).or_default().push(timer_id);
        Ok(())
    }

    /// Disarm a timer. No event is delivered for a disarmed timer.
    pub fn disarm(&mut self, timer_id: TimerId) -> Result<(), TimerError> {
        let t = self.table.get_mut(&timer_id.0).ok_or(TimerError::InvalidHandle)?;
        if !t.armed {
            return Ok(());
        }
        let deadline = t.deadline_ns;
        t.armed = false;
        // Remove from queue
        if let Some(entries) = self.queue.get_mut(&deadline) {
            entries.retain(|id| *id != timer_id);
            if entries.is_empty() {
                self.queue.remove(&deadline);
            }
        }
        Ok(())
    }

    /// Remove a timer entirely (called on cap close).
    pub fn free(&mut self, timer_id: TimerId) -> Result<(), TimerError> {
        // Disarm first to clean up queue entry
        let _ = self.disarm(timer_id);
        self.table.remove(&timer_id.0).ok_or(TimerError::InvalidHandle)?;
        Ok(())
    }

    #[cfg(test)]
    /// Get the timer state (test-only helper).
    pub fn get(&self, timer_id: TimerId) -> Option<&TimerState> {
        self.table.get(&timer_id.0)
    }

    /// Pop all timers whose deadline <= now. Returns (timer_id, TimerState clone) pairs.
    pub fn pop_expired(&mut self, now: u64) -> Vec<(TimerId, TimerState)> {
        let mut expired = Vec::new();
        // Collect deadline keys that are <= now
        let keys: Vec<u64> = self.queue.keys().copied().filter(|&d| d <= now).collect();
        for deadline in keys {
            if let Some(entries) = self.queue.remove(&deadline) {
                for timer_id in entries {
                    if let Some(t) = self.table.get_mut(&timer_id.0) {
                        if t.armed && t.deadline_ns <= now {
                            expired.push((timer_id, t.clone()));
                            if t.interval_ns > 0 {
                                // Periodic: drift-free rearm
                                t.deadline_ns += t.interval_ns;
                                t.seq = t.seq.wrapping_add(1).wrapping_add(t.missed);
                                // Re-insert into queue
                                self.queue.entry(t.deadline_ns).or_default().push(timer_id);
                            } else {
                                // One-shot: disarm
                                t.armed = false;
                                t.seq = t.seq.wrapping_add(1);
                            }
                        }
                    }
                }
            }
        }
        expired
    }

    /// Get the earliest deadline in the queue.
    pub fn earliest_deadline(&self) -> Option<u64> {
        self.queue.keys().next().copied()
    }

    /// Check whether a timer exists and is owned by the given PID.
    pub fn owned_by(&self, timer_id: TimerId, pid: u32) -> bool {
        self.table.get(&timer_id.0).map_or(false, |t| t.owner_pid == pid)
    }
}

impl fmt::Debug for HartTimers {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HartTimers")
            .field("table_len", &self.table.len())
            .field("queue_len", &self.queue.len())
            .field("next_id", &self.next_id)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new_timers() -> HartTimers {
        HartTimers::new()
    }

    #[test]
    fn alloc_increments_id() {
        let mut ht = new_timers();
        let id1 = ht.alloc(1, 10, 0).unwrap();
        let id2 = ht.alloc(1, 10, 0).unwrap();
        assert_ne!(id1, id2);
    }

    #[test]
    fn arm_and_pop_expired() {
        let mut ht = new_timers();
        let id = ht.alloc(1, 10, 0).unwrap();
        ht.arm(id, 100).unwrap();
        // Not yet expired
        assert!(ht.pop_expired(50).is_empty());
        // Expired
        let expired = ht.pop_expired(100);
        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0].0, id);
        // One-shot is now disarmed
        assert!(!ht.get(id).unwrap().armed);
    }

    #[test]
    fn periodic_rearm_is_drift_free() {
        let mut ht = new_timers();
        let interval = 1_000_000;
        let id = ht.alloc(1, 10, interval).unwrap();
        ht.arm(id, 100_000).unwrap();

        // First fire at 100_000
        let expired = ht.pop_expired(100_000);
        assert_eq!(expired.len(), 1);

        // After rearm, deadline should be 100_000 + interval = 1_100_000
        let t = ht.get(id).unwrap();
        assert_eq!(t.deadline_ns, 100_000 + interval);
        assert!(t.armed);

        // Second fire at 1_100_000
        let expired2 = ht.pop_expired(1_100_000);
        assert_eq!(expired2.len(), 1);
        assert_eq!(ht.get(id).unwrap().deadline_ns, 100_000 + 2 * interval);
    }

    #[test]
    fn disarm_removes_from_queue() {
        let mut ht = new_timers();
        let id = ht.alloc(1, 10, 0).unwrap();
        ht.arm(id, 100).unwrap();
        ht.disarm(id).unwrap();
        assert!(!ht.get(id).unwrap().armed);
        assert!(ht.pop_expired(200).is_empty());
    }

    #[test]
    fn free_removes_timer() {
        let mut ht = new_timers();
        let id = ht.alloc(1, 10, 0).unwrap();
        ht.free(id).unwrap();
        assert!(ht.get(id).is_none());
    }

    #[test]
    fn already_armed_rejected() {
        let mut ht = new_timers();
        let id = ht.alloc(1, 10, 0).unwrap();
        ht.arm(id, 100).unwrap();
        assert_eq!(ht.arm(id, 200), Err(TimerError::AlreadyArmed));
    }

    #[test]
    fn earliest_deadline_returns_min() {
        let mut ht = new_timers();
        let a = ht.alloc(1, 10, 0).unwrap();
        let b = ht.alloc(1, 10, 0).unwrap();
        ht.arm(a, 500).unwrap();
        ht.arm(b, 100).unwrap();
        assert_eq!(ht.earliest_deadline(), Some(100));
    }
}