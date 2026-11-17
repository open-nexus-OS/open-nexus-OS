// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: where a woken task runs (TASK-0363 P5) — on the hart where it runs soonest. Its home
//! (the hart it last ran on, cache-warm) when that hart takes it at once: asleep, about to hand
//! over to it (the waker's own hart, a call's server), or running a lower class it preempts.
//! Otherwise a free hart its affinity allows, its home's cluster first; only when there is none
//! does it queue at home. Pure and host-tested; `task::wake` feeds it the machine's state.
//! Before, a woken task always queued at home and an idle hart stole it at its next look (rate
//! gated, one a millisecond): a woken interactive task waited behind a same-class peer for the
//! peer's slice (board: 191 waits above 10 ms in 100 s, TASK-0363 P8).
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: below (host); `KSELFTEST: sched spread ok` (QEMU, the rule on the machine's
//!   topology)

use nexus_sched::CpuSet;

/// What a wake sees of the machine.
#[derive(Debug, Clone, Copy)]
pub struct Wake {
    /// The hart the task last ran on.
    pub home: usize,
    /// The hart that wakes it.
    pub waker: usize,
    /// The harts its affinity allows that are online.
    pub allowed: CpuSet,
    /// The harts asleep with no wake already sent to them.
    pub free: CpuSet,
    /// The harts of its home's cluster.
    pub near: CpuSet,
    /// Its home has a task of its class or above queued: it would wait behind it.
    pub backlog: bool,
    /// Its home runs a task of a lower class, which it preempts.
    pub home_runs_lower: bool,
}

/// The hart the woken task is queued on: always one `allowed` names (its home when nothing
/// else is), never a hart its affinity refuses.
pub fn place(w: &Wake) -> usize {
    let home_allowed = w.allowed.contains(w.home);
    let home_takes_it = w.free.contains(w.home) || w.home == w.waker || w.home_runs_lower;
    if home_allowed && home_takes_it && !w.backlog {
        return w.home;
    }
    let free = w.free.intersection(w.allowed);
    let queue_at = if home_allowed { Some(w.home) } else { w.allowed.lowest() };
    free.intersection(w.near).lowest().or_else(|| free.lowest()).or(queue_at).unwrap_or(w.home)
}

#[cfg(test)]
mod tests {
    use super::{place, Wake};
    use nexus_sched::CpuSet;

    fn set(bits: u64) -> CpuSet {
        CpuSet::from_bits(bits)
    }

    /// Two clusters of four; everything allowed; home 0 woken by hart 5.
    fn wake() -> Wake {
        Wake {
            home: 0,
            waker: 5,
            allowed: set(0xff),
            free: set(0),
            near: set(0x0f),
            backlog: false,
            home_runs_lower: false,
        }
    }

    /// A home that takes the task at once keeps it: asleep, the waker's own, or preempted.
    #[test]
    fn a_home_that_takes_it_at_once_keeps_it() {
        let free = Wake { free: set(0b1110_0001), ..wake() };
        assert_eq!(place(&free), 0);
        let handoff = Wake { waker: 0, free: set(0b0010), ..wake() };
        assert_eq!(place(&handoff), 0);
        let preempts = Wake { home_runs_lower: true, free: set(0b0010), ..wake() };
        assert_eq!(place(&preempts), 0);
    }

    /// Behind a peer at home, it goes to a free hart — its home's cluster first, else any.
    #[test]
    fn behind_a_peer_it_runs_on_a_free_hart_near_first() {
        let busy = Wake { free: set(0b0101_0100), ..wake() };
        assert_eq!(place(&busy), 2);
        let far = Wake { free: set(0b0100_0000), ..wake() };
        assert_eq!(place(&far), 6);
        let backlog = Wake { waker: 0, backlog: true, free: set(0b1000), ..wake() };
        assert_eq!(place(&backlog), 3);
        let none = Wake { free: set(0), ..wake() };
        assert_eq!(place(&none), 0);
    }

    /// Never on a hart its affinity refuses, free or not.
    #[test]
    fn test_reject_place_outside_affinity() {
        let pinned = Wake { allowed: set(0b0001), free: set(0b1110), ..wake() };
        assert_eq!(place(&pinned), 0);
        let elsewhere = Wake { allowed: set(0b1100_0000), free: set(0b0011_1110), ..wake() };
        let cpu = place(&elsewhere);
        assert!(set(0b1100_0000).contains(cpu), "placed on {cpu}");
        for free in 0..=0xffu64 {
            let w = Wake { allowed: set(0b0011_0000), free: set(free), ..wake() };
            assert!(set(0b0011_0000).contains(place(&w)), "free={free:#x}");
        }
    }

    /// Eight tasks woken in a row by hart 0, all homed there, spread over the eight harts: the
    /// first waits for the handoff, each next one takes a free hart, near ones first.
    #[test]
    fn eight_wakes_occupy_eight_harts() {
        let mut free = set(0xfe);
        let mut placed = CpuSet::from_bits(0);
        for n in 0..8 {
            let w = Wake { waker: 0, free, backlog: n > 0, ..wake() };
            let cpu = place(&w);
            assert!(!placed.contains(cpu), "hart {cpu} twice");
            placed = placed.union(CpuSet::single(cpu).unwrap());
            free = free.without(CpuSet::single(cpu).unwrap());
        }
        assert_eq!(placed.count(), 8);
    }
}
