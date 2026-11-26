// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the one deadline walk's decision for a blocked task (TASK-0363 P6/P7) — expired,
//! pending until when, or untimed — and the walk's gate, the earliest pending deadline. Pure and
//! host-tested; the walk (`syscall::api::expire_deadlines`) only acts on it. The deadline a wait
//! carries is read by `task::BlockReason::deadline_ns`, an exhaustive match: receive, send,
//! waitset and fence carry one, every other kind is untimed, and a new kind is decided there at
//! compile time. With it what a hart going to sleep arms (TASK-0363 P8): its own deadlines,
//! never another hart's.
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: below (host); `SELFTEST: wait deadline ok` (QEMU, a waitset's and a fence's)

/// What the deadline walk does with one blocked task at `now`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeadlineVerdict {
    /// No deadline: the walk leaves it, and it does not bound the walk's gate.
    Untimed,
    /// Pending until this time: the earliest of these is the next walk's gate.
    Pending(u64),
    /// Passed: the walk takes the task out of its wait and wakes it (it returns `TimedOut`).
    Expired,
}

/// The decision for a wait carrying `deadline` (`None`: its kind has none, or it is not
/// blocked; `Some(0)`: none asked for — wait forever). A deadline is passed AT its time
/// (`now >= deadline`), so the walk the gate admits at that very time finds it: a gate equal to
/// a pending deadline can never skip it. A wait its thread is still preparing (`blocked` false:
/// it blocks under no kernel lock, TASK-0330 P2) is pending at its deadline whatever the clock
/// says — only a blocked task can be taken out of its wait, and its hart commits or abandons
/// the wait at once: the walk that finds it blocked delivers it, and until then its deadline
/// keeps the gate.
pub const fn deadline_verdict(deadline: Option<u64>, now: u64, blocked: bool) -> DeadlineVerdict {
    match deadline {
        None | Some(0) => DeadlineVerdict::Untimed,
        Some(d) if blocked && now >= d => DeadlineVerdict::Expired,
        Some(d) => DeadlineVerdict::Pending(d),
    }
}

impl DeadlineVerdict {
    /// The deadline the walk's bounds keep for this wait: the earliest of these is the walk's
    /// gate after it ran (`trap::own_deadlines`), and its hart's own floor.
    pub const fn pending(self) -> Option<u64> {
        match self {
            Self::Pending(d) => Some(d),
            Self::Untimed | Self::Expired => None,
        }
    }
}

/// What a hart going to sleep writes into its timer (TASK-0363 P8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SleepArm {
    /// When the timer fires (ns).
    pub at: u64,
    /// It is the safety net (or the boot hart's heartbeat), not a deadline.
    pub net: bool,
}

/// The sleep arm of a hart whose own deadlines start at `own` (the earliest deadline of a task
/// that blocked on it or a timer cap armed on it — a lower bound, `u64::MAX`: none) and whose
/// net falls due at `net`: the own deadline when it comes first, else the net; `now` when either
/// is already due — a look became due between the idle check and here, and writing the timer to
/// the future would clear the interrupt pending for it. A hart arms only its own deadlines:
/// another hart's are that hart's (no backstop that wakes two harts for one deadline).
pub const fn sleep_arm(now: u64, own: u64, net: u64) -> SleepArm {
    if own <= now || net <= now {
        SleepArm { at: now, net: false }
    } else if own < net {
        SleepArm { at: own, net: false }
    } else {
        SleepArm { at: net, net: true }
    }
}

/// What a hart writes into its timer as user code runs next (TASK-0330 P4), in one unit with
/// `now`: the preemption tick `tick`, never later than its own deadlines starting at `own` (the
/// lower bound the sleep arm reads; `u64::MAX`: none) — `now` when one is already due, so the
/// interrupt fires in the user code, whose handler delivers it. Before, every dispatch wrote the
/// tick and left a due deadline out: a hart that switched tasks faster than the tick (two tasks
/// in an IPC exchange, each switch through the scheduler loop) never took its timer interrupt,
/// and no timer cap or deadline was delivered while it stayed busy — 6 s on one hart.
pub const fn dispatch_arm(now: u64, tick: u64, own: u64) -> u64 {
    if own <= now {
        now
    } else if own < tick {
        own
    } else {
        tick
    }
}

/// The next deadline of a periodic timer of `interval` that fell due at `deadline` and is
/// delivered at `now` (`deadline <= now`), and the whole periods that passed after `deadline`:
/// drift-free on its own phase and ahead of `now` — a timer delivered late fires once, says how
/// many periods it stands for, and skips them; one frame per period kept a 5 ms cadence trailing
/// its deadline by 0.8-6 s on one busy hart (2026-11-26). `interval` 0 (a one-shot): unchanged.
pub const fn periodic_rearm(deadline: u64, interval: u64, now: u64) -> (u64, u64) {
    if interval == 0 {
        return (deadline, 0);
    }
    let behind = now.saturating_sub(deadline) / interval;
    (deadline.saturating_add(interval.saturating_mul(behind.saturating_add(1))), behind)
}

#[cfg(test)]
mod tests {
    use super::{
        deadline_verdict, dispatch_arm, periodic_rearm, sleep_arm, DeadlineVerdict, SleepArm,
    };

    /// On time, a periodic timer moves one period on; delivered late, it fires once, counts
    /// the periods that passed and is ahead of the clock again on its own phase.
    #[test]
    fn test_reject_a_periodic_timer_that_trails_its_deadline() {
        let interval = 5_000_000;
        assert_eq!(periodic_rearm(100_000, interval, 100_000), (100_000 + interval, 0));
        assert_eq!(
            periodic_rearm(100_000, interval, 100_000 + interval - 1),
            (100_000 + interval, 0)
        );
        let now = 100_000 + 6_000_000_000 + 1_234;
        let (next, missed) = periodic_rearm(100_000, interval, now);
        assert_eq!(missed, 1_200);
        assert!(next > now && next <= now + interval);
        assert_eq!((next - 100_000) % interval, 0, "the timer keeps its phase");
        assert_eq!(periodic_rearm(7, 0, u64::MAX), (7, 0), "a one-shot is not moved");
    }

    /// User code runs until its tick — or until this hart's own next deadline, when that
    /// comes first.
    #[test]
    fn a_dispatch_arms_the_tick_or_an_earlier_own_deadline() {
        let (now, tick) = (1_000, 101_000);
        assert_eq!(dispatch_arm(now, tick, u64::MAX), tick);
        assert_eq!(dispatch_arm(now, tick, 50_000), 50_000);
        assert_eq!(dispatch_arm(now, tick, tick + 1), tick);
    }

    /// A deadline already due is never pushed out by a dispatch: the timer fires as the user
    /// code runs, however often the hart switches tasks.
    #[test]
    fn test_reject_a_dispatch_that_pushes_a_due_deadline_out() {
        let tick = 100_000;
        for now in [1_000, 50_000, 99_999] {
            for own in [0, 1, now - 1, now] {
                assert_eq!(dispatch_arm(now, now + tick, own), now);
            }
        }
        // The busy hart of the finding: a switch every 10 us for 6 s, a 5 ms deadline at
        // 5 ms — every dispatch after it arms `now`.
        let own = 5_000;
        let mut now = 0;
        while now < 6_000_000 {
            let at = dispatch_arm(now, now + tick, own);
            assert!(at <= own.max(now));
            now += 10;
        }
    }

    /// A hart with an own deadline before its net sleeps until that deadline; with none (or one
    /// after the net) until the net.
    #[test]
    fn a_sleeping_hart_wakes_for_its_own_deadline_else_its_net() {
        let (now, net) = (1_000, 500_001_000);
        assert_eq!(sleep_arm(now, 9_000, net), SleepArm { at: 9_000, net: false });
        assert_eq!(sleep_arm(now, u64::MAX, net), SleepArm { at: net, net: true });
        assert_eq!(sleep_arm(now, net + 1, net), SleepArm { at: net, net: true });
    }

    /// Never later than a pending own deadline, and never in the future when a deadline (or
    /// the heartbeat) is already due: the timer written past it would drop its interrupt.
    #[test]
    fn test_reject_sleep_past_a_due_or_pending_deadline() {
        let net = 500_000_000;
        for own in [1, 2_000, 499_999_999] {
            assert!(sleep_arm(0, own, net).at <= own);
        }
        assert_eq!(sleep_arm(5_000, 5_000, net), SleepArm { at: 5_000, net: false });
        assert_eq!(sleep_arm(5_000, 4_000, net), SleepArm { at: 5_000, net: false });
        assert_eq!(sleep_arm(5_000, u64::MAX, 5_000), SleepArm { at: 5_000, net: false });
    }

    /// A timed wait is pending until its deadline and passed at it — whatever its kind.
    #[test]
    fn a_timed_wait_expires_at_its_deadline() {
        let d = Some(5_000_000);
        assert_eq!(deadline_verdict(d, 0, true), DeadlineVerdict::Pending(5_000_000));
        assert_eq!(deadline_verdict(d, 4_999_999, true), DeadlineVerdict::Pending(5_000_000));
        assert_eq!(deadline_verdict(d, 5_000_000, true), DeadlineVerdict::Expired);
        assert_eq!(deadline_verdict(d, u64::MAX, true), DeadlineVerdict::Expired);
    }

    /// A wait still being prepared is never taken out of it — its thread is not blocked yet —
    /// and its deadline bounds the gate even when it has passed: the next walk looks again.
    #[test]
    fn test_reject_a_preparing_wait_expires_or_leaves_the_gate() {
        let d = Some(5_000_000);
        for now in [0, 5_000_000, u64::MAX] {
            let verdict = deadline_verdict(d, now, false);
            assert_eq!(verdict, DeadlineVerdict::Pending(5_000_000));
            assert_eq!(verdict.pending(), Some(5_000_000));
        }
        assert_eq!(deadline_verdict(Some(0), u64::MAX, false), DeadlineVerdict::Untimed);
    }

    /// A deadline of 0 is "wait forever", and a kind without a clock (a call, a child, a
    /// page-in) is never timed out — not even at the end of time.
    #[test]
    fn test_reject_untimed_waits_never_expire() {
        assert_eq!(deadline_verdict(Some(0), u64::MAX, true), DeadlineVerdict::Untimed);
        assert_eq!(deadline_verdict(None, u64::MAX, true), DeadlineVerdict::Untimed);
    }

    /// The gate is the earliest PENDING deadline: what expired and what is untimed do not
    /// bound it, and with nothing pending it is open (`u64::MAX`).
    #[test]
    fn the_gate_is_the_earliest_pending_deadline() {
        let now = 1_000;
        let waits = [Some(9_000), Some(4_000), Some(500), Some(0), None];
        let verdicts = waits.map(|d| deadline_verdict(d, now, true));
        assert_eq!(verdicts.iter().filter_map(|v| v.pending()).min(), Some(4_000));
        assert_eq!(verdicts.iter().filter(|v| **v == DeadlineVerdict::Expired).count(), 1);
        let quiet = [Some(0), None].map(|d| deadline_verdict(d, now, true));
        assert_eq!(quiet.iter().filter_map(|v| v.pending()).min(), None);
    }
}
