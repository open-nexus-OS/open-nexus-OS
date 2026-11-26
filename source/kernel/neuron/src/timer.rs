// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! RFC-0062: Kernel timer capability — per-hart timer table and deadline queue.
//!
//! CONTEXT: Manages timer objects created via `timer_create`. Each timer is bound
//! to a notification endpoint and delivers `OP_TIMER_FIRED` events on expiry.
//!
//! OWNERS: @kernel
//! STATUS: Draft (RFC-0062 Phase D.2)
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU selftests + boot markers
//!
//! INVARIANTS:
//! - Drift-free periodic rearm: `next_deadline += interval_ns * (1 + missed)`, never
//!   `now + interval` — a timer delivered late skips to its next slot ahead of `now`
//! - Coalescing: at most one pending event per timer; `missed` counter for skipped ticks
//! - Bounded state: fixed-size table (MAX_TIMERS = 64), O(log n) queue operations
//! - Cap lifecycle: closing a timer cap removes it from all queues
//! - The table is behind its own lock (class `ipc`, RFC-0102; TASK-0330 P4): a hart delivers
//!   what fired under no kernel lock. The gates (`NEXT_CAP_NS`, each hart's floor) are lowered
//!   and set exact inside that lock.
//! - A delivery is one hold of that lock, from the pop to the last event sent
//!   ([`HartTimers::deliver`]): one timer's events reach its endpoint in the order they fired,
//!   whichever harts deliver, and an endpoint's death ([`HartTimers::forget_endpoint`]) comes
//!   after every fire popped for it — its id is given out again. So the table is the first of
//!   the IPC objects in their order (key 0; an endpoint is keyed by its address): a path that
//!   holds an endpoint, a waitset or a fence takes no timer.

extern crate alloc;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::fmt;

use crate::sync::lock_rank::Class;
use crate::sync::ranked::RankedLock;

/// Maximum number of concurrent timers per hart.
pub(crate) const MAX_TIMERS: usize = 64;

/// The earliest armed timer-cap deadline (ns; `u64::MAX`: none) — a lower bound read without
/// the kernel lock by the idle hart deciding whether its delivery is due (TASK-0363 P6/P4).
/// Lowered by every arm, set exactly by every expiry pass; a bound too low costs one locked
/// look, a bound too high would lose an expiry, which these two rules prevent.
static NEXT_CAP_NS: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(u64::MAX);

/// The earliest armed timer-cap deadline, or `u64::MAX` (see [`NEXT_CAP_NS`]).
pub(crate) fn next_cap_ns() -> u64 {
    NEXT_CAP_NS.load(core::sync::atomic::Ordering::Acquire)
}

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
    /// The CPU it was armed on — whose timer wakes for it (TASK-0363 P8).
    pub arm_cpu: u32,
}

/// The timers, behind their lock: every operation is one hold of it.
pub struct HartTimers {
    inner: RankedLock<Timers>,
}

impl HartTimers {
    pub fn new() -> Self {
        let inner = RankedLock::new(Class::Ipc, Timers::new());
        // The first of the IPC objects: a delivery sends while it holds the table.
        inner.set_key(0);
        Self { inner }
    }

    /// Allocate a new timer slot. Returns the timer ID.
    pub fn alloc(
        &self,
        owner_pid: u32,
        notify_ep: u32,
        interval_ns: u64,
    ) -> Result<TimerId, TimerError> {
        self.inner.lock().alloc(owner_pid, notify_ep, interval_ns)
    }

    /// Arm a timer with an absolute deadline, on CPU `cpu` (whose timer wakes for it).
    pub fn arm(&self, timer_id: TimerId, deadline_ns: u64, cpu: u32) -> Result<(), TimerError> {
        self.inner.lock().arm(timer_id, deadline_ns, cpu)
    }

    /// Disarm a timer. An event already sent stays queued on its endpoint.
    pub fn disarm(&self, timer_id: TimerId) -> Result<(), TimerError> {
        self.inner.lock().disarm(timer_id)
    }

    /// Remove a timer entirely (called on cap close).
    pub fn free(&self, timer_id: TimerId) -> Result<(), TimerError> {
        self.inner.lock().free(timer_id)
    }

    /// Pops every timer whose deadline <= now and hands each to `send`, in one hold of the
    /// table: `send` takes the timer's endpoint and what a wake takes (a thread, a run queue),
    /// never a waitset, a fence or a timer. Returns how many fired.
    pub fn deliver(&self, now: u64, mut send: impl FnMut(TimerId, &TimerState)) -> usize {
        let mut timers = self.inner.lock();
        let fired = timers.pop_expired(now);
        for (id, state) in &fired {
            send(*id, state);
        }
        fired.len()
    }

    #[cfg(test)]
    /// The timers [`Self::deliver`] would hand out at `now` (test-only helper).
    pub fn pop_expired(&self, now: u64) -> Vec<(TimerId, TimerState)> {
        let mut fired = Vec::new();
        self.deliver(now, |id, state| fired.push((id, state.clone())));
        fired
    }

    /// Check whether a timer exists and is owned by the given PID.
    pub fn owned_by(&self, timer_id: TimerId, pid: u32) -> bool {
        self.inner.lock().owned_by(timer_id, pid)
    }

    /// Frees every timer `pid` owns — its exit (TASK-0333 6a: a task's objects die with it,
    /// as a `cap_close` of each would free them). Returns how many.
    pub fn free_owned_by(&self, pid: u32) -> usize {
        self.inner.lock().free_owned_by(pid)
    }

    /// Frees every timer that notifies `endpoint` — the endpoint died, so a fire would write
    /// into whatever its slot holds next. A delivery is one hold of the table, so every fire
    /// popped for the endpoint was sent before this returns. Returns how many timers.
    pub fn forget_endpoint(&self, endpoint: u32) -> usize {
        self.inner.lock().forget_endpoint(endpoint)
    }

    /// Live timers (the record).
    pub fn len(&self) -> usize {
        self.inner.lock().len()
    }

    #[cfg(test)]
    /// Get the timer state (test-only helper).
    pub fn get(&self, timer_id: TimerId) -> Option<TimerState> {
        self.inner.lock().get(timer_id).cloned()
    }

    #[cfg(test)]
    pub fn earliest_deadline(&self) -> Option<u64> {
        self.inner.lock().earliest_deadline()
    }
}

/// The timer table and its deadline queue.
struct Timers {
    /// Timer table: timer_id -> TimerState.
    table: BTreeMap<u32, TimerState>,
    /// Deadline queue: deadline_ns -> Vec<TimerId>.
    /// Sorted ascending by deadline_ns. Multiple timers may share the same deadline.
    queue: BTreeMap<u64, Vec<TimerId>>,
    /// Next available timer ID.
    next_id: u32,
}

impl Timers {
    const fn new() -> Self {
        Self { table: BTreeMap::new(), queue: BTreeMap::new(), next_id: 1 }
    }

    /// Allocate a new timer slot. Returns the timer ID.
    pub fn alloc(
        &mut self,
        owner_pid: u32,
        notify_ep: u32,
        interval_ns: u64,
    ) -> Result<TimerId, TimerError> {
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
        self.table.insert(
            id,
            TimerState {
                owner_pid,
                notify_ep,
                deadline_ns: 0,
                interval_ns,
                seq: 0,
                missed: 0,
                armed: false,
                arm_cpu: 0,
            },
        );
        Ok(TimerId(id))
    }

    /// Arm a timer with an absolute deadline, on CPU `cpu` (whose timer wakes for it).
    pub fn arm(&mut self, timer_id: TimerId, deadline_ns: u64, cpu: u32) -> Result<(), TimerError> {
        let t = self.table.get_mut(&timer_id.0).ok_or(TimerError::InvalidHandle)?;
        if t.armed {
            return Err(TimerError::AlreadyArmed);
        }
        t.deadline_ns = deadline_ns;
        t.armed = true;
        t.arm_cpu = cpu;
        self.queue.entry(deadline_ns).or_default().push(timer_id);
        NEXT_CAP_NS.fetch_min(deadline_ns, core::sync::atomic::Ordering::AcqRel);
        use crate::trap::own_deadlines as own;
        own::lower(own::Kind::Cap, cpu as usize, deadline_ns);
        Ok(())
    }

    /// The gates are exact again: the earliest armed deadline, and each hart's own (TASK-0363
    /// P8) — set together, so a hart's own floor is never below the global one for long.
    fn publish_gates(&self) {
        NEXT_CAP_NS.store(
            self.earliest_deadline().unwrap_or(u64::MAX),
            core::sync::atomic::Ordering::Release,
        );
        use crate::trap::own_deadlines as own;
        own::begin(own::Kind::Cap);
        for (cpu, deadline) in self.armed() {
            own::note(own::Kind::Cap, cpu as usize, deadline);
        }
        own::publish(own::Kind::Cap);
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
        // Exact again: a cancelled pacer timer does not leave a gate that wakes a locked look.
        self.publish_gates();
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
                            // The frame says how many periods it stands for (`missed`): a
                            // periodic timer delivered late skips them, ahead of `now` on its own
                            // phase (`deadline_rule::periodic_rearm`, host-tested).
                            let (next, behind) = crate::deadline_rule::periodic_rearm(
                                t.deadline_ns,
                                t.interval_ns,
                                now,
                            );
                            t.missed = u32::try_from(behind).unwrap_or(u32::MAX);
                            expired.push((timer_id, t.clone()));
                            if t.interval_ns > 0 {
                                // Periodic: drift-free rearm.
                                t.deadline_ns = next;
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
        self.publish_gates();
        expired
    }

    /// Get the earliest deadline in the queue.
    pub fn earliest_deadline(&self) -> Option<u64> {
        self.queue.keys().next().copied()
    }

    /// Every armed timer as `(cpu it was armed on, deadline)` — each hart's own floor
    /// (TASK-0363 P8; at most [`MAX_TIMERS`]).
    pub fn armed(&self) -> impl Iterator<Item = (u32, u64)> + '_ {
        self.table.values().filter(|t| t.armed).map(|t| (t.arm_cpu, t.deadline_ns))
    }

    /// Check whether a timer exists and is owned by the given PID.
    pub fn owned_by(&self, timer_id: TimerId, pid: u32) -> bool {
        self.table.get(&timer_id.0).is_some_and(|t| t.owner_pid == pid)
    }

    /// Frees every timer `pid` owns — its exit (TASK-0333 6a: a task's objects die with it,
    /// as a `cap_close` of each would free them). Returns how many.
    pub fn free_owned_by(&mut self, pid: u32) -> usize {
        let ids: Vec<u32> =
            self.table.iter().filter(|(_, t)| t.owner_pid == pid).map(|(id, _)| *id).collect();
        for id in &ids {
            let _ = self.free(TimerId(*id));
        }
        ids.len()
    }

    /// Frees every timer that notifies `endpoint` — the endpoint died, so a fire would write
    /// into whatever its slot holds next. Returns how many.
    pub fn forget_endpoint(&mut self, endpoint: u32) -> usize {
        let ids: Vec<u32> =
            self.table.iter().filter(|(_, t)| t.notify_ep == endpoint).map(|(id, _)| *id).collect();
        for id in &ids {
            let _ = self.free(TimerId(*id));
        }
        ids.len()
    }

    /// Live timers (the record).
    pub fn len(&self) -> usize {
        self.table.len()
    }
}

impl fmt::Debug for HartTimers {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let timers = self.inner.lock();
        f.debug_struct("HartTimers")
            .field("table_len", &timers.table.len())
            .field("queue_len", &timers.queue.len())
            .field("next_id", &timers.next_id)
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
        let ht = new_timers();
        let id1 = ht.alloc(1, 10, 0).unwrap();
        let id2 = ht.alloc(1, 10, 0).unwrap();
        assert_ne!(id1, id2);
    }

    #[test]
    fn arm_and_pop_expired() {
        let ht = new_timers();
        let id = ht.alloc(1, 10, 0).unwrap();
        ht.arm(id, 100, 0).unwrap();
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
        let ht = new_timers();
        let interval = 1_000_000;
        let id = ht.alloc(1, 10, interval).unwrap();
        ht.arm(id, 100_000, 0).unwrap();

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
        let ht = new_timers();
        let id = ht.alloc(1, 10, 0).unwrap();
        ht.arm(id, 100, 0).unwrap();
        ht.disarm(id).unwrap();
        assert!(!ht.get(id).unwrap().armed);
        assert!(ht.pop_expired(200).is_empty());
    }

    #[test]
    fn free_removes_timer() {
        let ht = new_timers();
        let id = ht.alloc(1, 10, 0).unwrap();
        ht.free(id).unwrap();
        assert!(ht.get(id).is_none());
    }

    #[test]
    fn already_armed_rejected() {
        let ht = new_timers();
        let id = ht.alloc(1, 10, 0).unwrap();
        ht.arm(id, 100, 0).unwrap();
        assert_eq!(ht.arm(id, 200, 0), Err(TimerError::AlreadyArmed));
    }

    #[test]
    fn earliest_deadline_returns_min() {
        let ht = new_timers();
        let a = ht.alloc(1, 10, 0).unwrap();
        let b = ht.alloc(1, 10, 0).unwrap();
        ht.arm(a, 500, 0).unwrap();
        ht.arm(b, 100, 0).unwrap();
        assert_eq!(ht.earliest_deadline(), Some(100));
    }
}
