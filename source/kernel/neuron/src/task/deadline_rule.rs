// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the one deadline walk's decision for a blocked task (TASK-0363 P6/P7) — expired,
//! pending until when, or untimed — and the walk's gate, the earliest pending deadline. Pure and
//! host-tested; the walk (`syscall::api::expire_deadlines`) only acts on it. The deadline a wait
//! carries is read by `task::BlockReason::deadline_ns`, an exhaustive match: receive, send,
//! waitset and fence carry one, every other kind is untimed, and a new kind is decided there at
//! compile time.
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
/// a pending deadline can never skip it.
pub const fn deadline_verdict(deadline: Option<u64>, now: u64) -> DeadlineVerdict {
    match deadline {
        None | Some(0) => DeadlineVerdict::Untimed,
        Some(d) if now >= d => DeadlineVerdict::Expired,
        Some(d) => DeadlineVerdict::Pending(d),
    }
}

/// The walk's gate after it ran: the earliest deadline still pending (`u64::MAX`: none).
pub fn next_gate(gate: u64, verdict: DeadlineVerdict) -> u64 {
    match verdict {
        DeadlineVerdict::Pending(d) => gate.min(d),
        DeadlineVerdict::Untimed | DeadlineVerdict::Expired => gate,
    }
}

#[cfg(test)]
mod tests {
    use super::{deadline_verdict, next_gate, DeadlineVerdict};

    /// A timed wait is pending until its deadline and passed at it — whatever its kind.
    #[test]
    fn a_timed_wait_expires_at_its_deadline() {
        let d = Some(5_000_000);
        assert_eq!(deadline_verdict(d, 0), DeadlineVerdict::Pending(5_000_000));
        assert_eq!(deadline_verdict(d, 4_999_999), DeadlineVerdict::Pending(5_000_000));
        assert_eq!(deadline_verdict(d, 5_000_000), DeadlineVerdict::Expired);
        assert_eq!(deadline_verdict(d, u64::MAX), DeadlineVerdict::Expired);
    }

    /// A deadline of 0 is "wait forever", and a kind without a clock (a call, a child, a
    /// page-in) is never timed out — not even at the end of time.
    #[test]
    fn test_reject_untimed_waits_never_expire() {
        assert_eq!(deadline_verdict(Some(0), u64::MAX), DeadlineVerdict::Untimed);
        assert_eq!(deadline_verdict(None, u64::MAX), DeadlineVerdict::Untimed);
    }

    /// The gate is the earliest PENDING deadline: what expired and what is untimed do not
    /// bound it, and with nothing pending it is open (`u64::MAX`).
    #[test]
    fn the_gate_is_the_earliest_pending_deadline() {
        let now = 1_000;
        let waits = [Some(9_000), Some(4_000), Some(500), Some(0), None];
        let verdicts = waits.map(|d| deadline_verdict(d, now));
        assert_eq!(verdicts.iter().fold(u64::MAX, |g, v| next_gate(g, *v)), 4_000);
        assert_eq!(verdicts.iter().filter(|v| **v == DeadlineVerdict::Expired).count(), 1);
        let quiet = [Some(0), None].map(|d| deadline_verdict(d, now));
        assert_eq!(quiet.iter().fold(u64::MAX, |g, v| next_gate(g, *v)), u64::MAX);
    }
}
