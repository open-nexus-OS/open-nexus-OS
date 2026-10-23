// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: who grew (TASK-0333 6b-3). At each reap the kernel measures every live address
//! space's committed bytes against the high-water it last reported for that process and names
//! the ones that reached a new high by at least [`STEP`]: `KINIT: mm grew (pid= sid= rss= by=)`.
//! A service whose heap keeps growing over a session — a leak, or an allocator that never frees —
//! is named by this line; a fluctuation below its high is not. The first sight of a process sets
//! its mark silently. This module is the rule and its table, pure (host-tested); `mm::usage`
//! feeds it the spaces.
//!
//! NOT target-gated (pure index logic, like `lazy` and `vmo_lifetime`).
//!
//! OWNERS: @kernel-mm-team
//! STATUS: Functional (TASK-0333 6b-3)
//! API_STABILITY: Internal
//! TEST_COVERAGE: unit tests below — the first sight, the step, a fluctuation, a pid reused by
//!   another service, a pid past the table

/// The growth a new high must reach to be named.
pub const STEP: u64 = 256 * 1024;

/// Processes followed: pids below this (the services start first; an app past it is not named).
pub const TRACKED: usize = 256;

#[derive(Clone, Copy)]
struct Mark {
    sid: u64,
    high: u64,
    set: bool,
}

/// The high-water of each followed process, by pid.
pub struct Marks {
    marks: [Mark; TRACKED],
}

impl Marks {
    /// Nothing seen yet.
    #[must_use]
    pub const fn new() -> Self {
        Self { marks: [Mark { sid: 0, high: 0, set: false }; TRACKED] }
    }

    /// What to name for process `pid` of service `sid` holding `rss` committed bytes now: the
    /// growth since its high-water when it reached a new high by [`STEP`] or more (the mark moves
    /// up to `rss`), `None` otherwise. A process seen first, or a pid now another service's, sets
    /// its mark silently; a pid past [`TRACKED`] is not followed.
    pub fn note(&mut self, pid: u32, sid: u64, rss: u64) -> Option<u64> {
        let mark = self.marks.get_mut(pid as usize)?;
        if !mark.set || mark.sid != sid {
            *mark = Mark { sid, high: rss, set: true };
            return None;
        }
        let by = rss.checked_sub(mark.high).filter(|&by| by >= STEP)?;
        mark.high = rss;
        Some(by)
    }
}

impl Default for Marks {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SID: u64 = 0x600e_3345_cc71_04bb;

    /// The first sight sets the mark silently; growth below the step is quiet; a new high by the
    /// step is named with the growth since the last high, and the mark moves there.
    #[test]
    fn a_new_high_by_the_step_is_named_once() {
        let mut marks = Marks::new();
        assert_eq!(marks.note(7, SID, 4 << 20), None, "first sight");
        assert_eq!(marks.note(7, SID, (4 << 20) + STEP - 4096), None, "below the step");
        assert_eq!(marks.note(7, SID, (4 << 20) + STEP), Some(STEP));
        assert_eq!(marks.note(7, SID, (4 << 20) + STEP), None, "named once");
        assert_eq!(marks.note(7, SID, (5 << 20) + STEP), Some(1 << 20));
    }

    /// A space that shrank and grew back to its old high has not grown: only a new high counts.
    #[test]
    fn a_fluctuation_below_the_high_is_not_growth() {
        let mut marks = Marks::new();
        assert_eq!(marks.note(3, SID, 8 << 20), None);
        assert_eq!(marks.note(3, SID, 2 << 20), None, "shrank");
        assert_eq!(marks.note(3, SID, 8 << 20), None, "back at its high");
        assert_eq!(marks.note(3, SID, (8 << 20) + STEP), Some(STEP));
    }

    /// A pid another service now holds starts over, silently; a pid past the table is not
    /// followed.
    #[test]
    fn test_reject_naming_a_reused_pid_or_one_past_the_table() {
        let mut marks = Marks::new();
        assert_eq!(marks.note(9, SID, 1 << 20), None);
        assert_eq!(marks.note(9, SID ^ 1, 64 << 20), None, "another service's pid: a new mark");
        assert_eq!(marks.note(9, SID ^ 1, (64 << 20) + STEP), Some(STEP));
        assert_eq!(marks.note(TRACKED as u32, SID, 0), None);
        assert_eq!(marks.note(TRACKED as u32, SID, 1 << 30), None, "never followed");
    }
}
