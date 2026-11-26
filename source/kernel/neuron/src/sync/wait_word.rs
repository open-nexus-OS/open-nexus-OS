// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: how a thread blocks and how it is woken — the rule (TASK-0330 P2, ADR-0102). A
//! thread blocks in two steps: it says it is about to wait (`prepare`), registers on what it
//! waits for and looks once more, and only then leaves its hart (`commit`). A wake that falls
//! between the two steps is kept in the word, so the commit finds it and the thread runs on —
//! no wake is lost, and no waker ever waits for a thread to finish blocking. The word is read
//! and changed under the thread's lock; this file is the transitions alone.
//! OWNERS: @kernel-sched-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: host (`tests::*`: every order of the three steps and any number of wakes)
//! INVARIANTS: a wake after `prepare` and before `commit` makes the commit answer `Woken`; a
//!   blocked thread is made runnable by exactly one wake; a wake of a thread that is not
//!   waiting changes nothing.
//! ADR: docs/adr/0102-a-thread-blocks-in-two-steps-and-a-waker-never-waits-ipc-leaves-the-kernel-lock.md

/// Where a thread is between running and waiting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wait {
    /// It runs, or is queued to run.
    Running,
    /// It said it is about to wait and is registering on what it waits for.
    Preparing,
    /// It left its hart; a wake makes it runnable.
    Blocked,
    /// A wake found it preparing: its commit will not block.
    Woken,
}

/// What the second step of a block did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Commit {
    /// The thread is blocked: the caller takes it off its hart in the same hold.
    Blocked,
    /// A wake came first (or the thread never prepared): it runs on and looks again.
    Woken,
}

/// What a wake found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wake {
    /// The thread is not waiting: a wake is a hint, nothing to do.
    Nothing,
    /// It was preparing: the wake is kept for its commit.
    Noted,
    /// It was blocked: the waker makes it runnable in the same hold.
    Run,
}

impl Wait {
    /// The first step: the thread is about to wait. A wake kept from an earlier wait that the
    /// thread left on its own is forgotten here — it belonged to that wait.
    #[must_use]
    pub const fn prepare(self) -> Self {
        Self::Preparing
    }

    /// The second step: the word after it, and whether the thread blocked.
    #[must_use]
    pub const fn commit(self) -> (Self, Commit) {
        match self {
            Self::Preparing => (Self::Blocked, Commit::Blocked),
            Self::Running | Self::Blocked | Self::Woken => (Self::Running, Commit::Woken),
        }
    }

    /// A wake: the word after it, and what the waker does.
    #[must_use]
    pub const fn wake(self) -> (Self, Wake) {
        match self {
            Self::Blocked => (Self::Running, Wake::Run),
            Self::Preparing => (Self::Woken, Wake::Noted),
            Self::Running | Self::Woken => (self, Wake::Nothing),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_thread_that_prepared_and_committed_is_woken_once() {
        let word = Wait::Running.prepare();
        let (word, commit) = word.commit();
        assert_eq!((word, commit), (Wait::Blocked, Commit::Blocked));
        let (word, first) = word.wake();
        assert_eq!((word, first), (Wait::Running, Wake::Run));
        assert_eq!(word.wake(), (Wait::Running, Wake::Nothing), "the second wake is a hint");
    }

    #[test]
    fn a_wake_between_the_two_steps_keeps_the_thread_running() {
        let (word, wake) = Wait::Running.prepare().wake();
        assert_eq!((word, wake), (Wait::Woken, Wake::Noted));
        assert_eq!(word.wake(), (Wait::Woken, Wake::Nothing), "one wake is enough");
        assert_eq!(word.commit(), (Wait::Running, Commit::Woken));
    }

    #[test]
    fn test_reject_a_block_that_was_never_prepared() {
        assert_eq!(Wait::Running.commit(), (Wait::Running, Commit::Woken));
        assert_eq!(Wait::Woken.commit(), (Wait::Running, Commit::Woken));
    }

    #[test]
    fn a_wake_kept_from_a_wait_the_thread_left_does_not_reach_the_next_wait() {
        let (word, _) = Wait::Running.prepare().wake();
        // The thread found its answer and left the wait without a commit; its next wait starts
        // clean and blocks.
        assert_eq!(word.prepare().commit(), (Wait::Blocked, Commit::Blocked));
    }

    /// Every order of wakes around the two steps: a wake after the prepare is never lost, and a
    /// blocked thread is run by exactly one waker.
    #[test]
    fn no_wake_is_lost_in_any_order() {
        // Bit i of `plan` set: a wake falls before step i (0: before prepare, 1: between the
        // steps, 2: after the commit, 3: once more).
        for plan in 0u8..16 {
            let wake_at = |step: u8| plan & (1 << step) != 0;
            let mut word = Wait::Running;
            let mut runs = 0;
            let mut note = |word: &mut Wait| {
                let (next, wake) = word.wake();
                *word = next;
                runs += usize::from(wake == Wake::Run);
            };
            if wake_at(0) {
                note(&mut word);
            }
            word = word.prepare();
            if wake_at(1) {
                note(&mut word);
            }
            let (next, commit) = word.commit();
            word = next;
            assert_eq!(commit == Commit::Woken, wake_at(1), "plan {plan:#06b}");
            for step in 2..4 {
                if wake_at(step) {
                    note(&mut word);
                }
            }
            let blocked_then_woken = !wake_at(1) && (wake_at(2) || wake_at(3));
            assert_eq!(runs, usize::from(blocked_then_woken), "plan {plan:#06b}");
            let still_blocked = !wake_at(1) && !wake_at(2) && !wake_at(3);
            assert_eq!(word == Wait::Blocked, still_blocked, "plan {plan:#06b}");
        }
    }
}
