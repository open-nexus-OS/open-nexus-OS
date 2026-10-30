// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: A text field's input session (RFC-0113 §6, TASK-0346): what the focused field applied
//! and how long each key took from the system's first sight of it (`t_ns`, stamped at the source)
//! to the present that showed it. One aggregate per session, when the focus leaves the field —
//! never a line per key (a line's count and timing are a typed word's length and rhythm), never
//! any line for a secure field (a password's length never leaves the field). Fixed storage: the
//! session allocates nothing.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: the unit tests below (the summary, percentiles, a field change, `test_reject_*`
//!   for a secure session and an empty one)

/// Key latencies one session keeps (the newest win; `max_us` keeps the worst of all).
pub const SESSION_SAMPLES: usize = 128;
/// Applied keys waiting for the present that shows them.
const PENDING_MAX: usize = 32;

/// What the field applied.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Applied {
    /// A committed character.
    Char,
    /// An editing action (a move, a deletion, a selection, a clipboard command).
    Action,
}

/// One finished session's aggregate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SessionSummary {
    pub chars: u32,
    pub actions: u32,
    /// Keys whose latency was measured (a key no device produced is untimed).
    pub timed: u32,
    pub p50_us: u32,
    pub p99_us: u32,
    pub max_us: u32,
}

impl SessionSummary {
    /// The session's verdict: its p99 key → present against the input budget (RFC-0108) — one
    /// slow key in a long session is its `max_us`, not its verdict. `None` when no key was timed:
    /// there is nothing to judge, and nothing is `ok` without a measurement.
    #[must_use]
    pub const fn verdict(&self) -> Option<crate::Verdict> {
        if self.timed == 0 {
            None
        } else {
            Some(crate::judge_input(self.p99_us as u64))
        }
    }
}

/// The running session of one focused field.
#[derive(Clone, Debug)]
pub struct InputSession {
    field: Option<usize>,
    secure: bool,
    chars: u32,
    actions: u32,
    pending: [u64; PENDING_MAX],
    pending_n: usize,
    samples: [u32; SESSION_SAMPLES],
    samples_n: usize,
    next: usize,
    timed: u32,
    max_us: u32,
}

impl Default for InputSession {
    fn default() -> Self {
        Self::new()
    }
}

impl InputSession {
    pub const fn new() -> Self {
        Self {
            field: None,
            secure: false,
            chars: 0,
            actions: 0,
            pending: [0; PENDING_MAX],
            pending_n: 0,
            samples: [0; SESSION_SAMPLES],
            samples_n: 0,
            next: 0,
            timed: 0,
            max_us: 0,
        }
    }

    /// The field focused now (`Some((field, secure))`, `None` = none). A change ends the running
    /// session: its summary comes back — `None` for a secure session or one that applied
    /// nothing — and a new one starts.
    pub fn track(&mut self, focus: Option<(usize, bool)>) -> Option<SessionSummary> {
        if focus.map(|(field, _)| field) == self.field {
            return None;
        }
        let summary = self.summary();
        *self = Self::new();
        if let Some((field, secure)) = focus {
            self.field = Some(field);
            self.secure = secure;
        }
        summary
    }

    /// A key the field applied, first seen at `t_ns` (0 = no device produced it: untimed).
    pub fn applied(&mut self, what: Applied, t_ns: u64) {
        if self.field.is_none() {
            return;
        }
        match what {
            Applied::Char => self.chars = self.chars.saturating_add(1),
            Applied::Action => self.actions = self.actions.saturating_add(1),
        }
        if t_ns != 0 && self.pending_n < PENDING_MAX {
            self.pending[self.pending_n] = t_ns;
            self.pending_n += 1;
        }
    }

    /// A key the field applied that shows nothing (a copy, a move at the end) — answered at
    /// `now_ns`, not at a later present it does not need.
    pub fn applied_now(&mut self, what: Applied, t_ns: u64, now_ns: u64) {
        if self.field.is_none() {
            return;
        }
        match what {
            Applied::Char => self.chars = self.chars.saturating_add(1),
            Applied::Action => self.actions = self.actions.saturating_add(1),
        }
        if t_ns != 0 {
            self.sample(now_ns.saturating_sub(t_ns));
        }
    }

    /// The present showing everything applied so far left at `now_ns`: each pending key's latency
    /// is taken.
    pub fn presented(&mut self, now_ns: u64) {
        for i in 0..self.pending_n {
            self.sample(now_ns.saturating_sub(self.pending[i]));
        }
        self.pending_n = 0;
    }

    fn sample(&mut self, ns: u64) {
        let us = u32::try_from(ns / 1_000).unwrap_or(u32::MAX);
        self.samples[self.next] = us;
        self.next = (self.next + 1) % SESSION_SAMPLES;
        self.samples_n = (self.samples_n + 1).min(SESSION_SAMPLES);
        self.timed = self.timed.saturating_add(1);
        self.max_us = self.max_us.max(us);
    }

    fn summary(&self) -> Option<SessionSummary> {
        if self.field.is_none() || self.secure || self.chars + self.actions == 0 {
            return None;
        }
        let mut sorted = [0u32; SESSION_SAMPLES];
        sorted[..self.samples_n].copy_from_slice(&self.samples[..self.samples_n]);
        let kept = &mut sorted[..self.samples_n];
        kept.sort_unstable();
        let at =
            |q: usize| kept.get((kept.len().saturating_sub(1) * q) / 100).copied().unwrap_or(0);
        Some(SessionSummary {
            chars: self.chars,
            actions: self.actions,
            timed: self.timed,
            p50_us: at(50),
            p99_us: at(99),
            max_us: self.max_us,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_session_is_summarized_when_the_focus_leaves() {
        let mut s = InputSession::new();
        assert_eq!(s.track(Some((7, false))), None, "a new session has nothing to say");
        for (i, t) in [1_000_000u64, 1_100_000, 1_200_000].iter().enumerate() {
            s.applied(if i == 2 { Applied::Action } else { Applied::Char }, *t);
        }
        s.presented(1_250_000);
        let sum = s.track(None).expect("the field had input");
        assert_eq!((sum.chars, sum.actions, sum.timed), (2, 1, 3));
        assert_eq!(sum.max_us, 250);
        assert_eq!(sum.p50_us, 150);
        assert_eq!(s.track(None), None, "said once");
    }

    #[test]
    fn a_field_change_ends_one_session_and_starts_the_next() {
        let mut s = InputSession::new();
        s.track(Some((1, false)));
        s.applied(Applied::Char, 10_000);
        s.presented(20_000);
        let first = s.track(Some((2, false))).expect("field 1's session");
        assert_eq!(first.chars, 1);
        s.applied(Applied::Char, 0);
        s.presented(50_000);
        let second = s.track(None).expect("field 2's session");
        assert_eq!((second.chars, second.timed), (1, 0), "a key no device produced is untimed");
    }

    #[test]
    fn percentiles_come_from_the_newest_samples_and_the_worst_stays() {
        let mut s = InputSession::new();
        s.track(Some((3, false)));
        s.applied(Applied::Char, 1);
        s.presented(900_000_001); // one terrible key first
        for k in 0..(SESSION_SAMPLES as u64 + 40) {
            s.applied(Applied::Char, 1_000_000 * (k + 1));
            s.presented(1_000_000 * (k + 1) + 2_000);
        }
        let sum = s.track(None).expect("summary");
        assert_eq!(sum.p99_us, 2, "the window holds only the newest");
        assert_eq!(sum.max_us, 900_000, "the worst of all stays");
    }

    #[test]
    fn test_reject_ok_for_a_session_without_a_timed_key_or_past_the_budget() {
        let mut s = InputSession::new();
        s.track(Some((5, false)));
        s.applied(Applied::Char, 0); // an input method's commit: no device stamp
        s.presented(10_000);
        assert_eq!(s.track(None).expect("one char").verdict(), None, "nothing measured");
        s.track(Some((6, false)));
        s.applied(Applied::Char, 1_000);
        s.presented(1_000 + 120_000_000);
        let slow = s.track(None).expect("one slow key");
        assert_eq!(slow.verdict(), Some(crate::Verdict::Fail));
        s.track(Some((7, false)));
        s.applied(Applied::Char, 1_000);
        s.presented(1_000 + 8_000_000);
        assert_eq!(s.track(None).expect("one quick key").verdict(), Some(crate::Verdict::Ok));
    }

    #[test]
    fn test_reject_a_summary_of_a_secure_field() {
        let mut s = InputSession::new();
        s.track(Some((9, true)));
        for k in 1..=8u64 {
            s.applied(Applied::Char, k * 1_000);
        }
        s.presented(20_000);
        assert_eq!(s.track(None), None, "a password's session never leaves the field");
    }

    #[test]
    fn test_reject_a_summary_of_an_empty_session_or_input_without_a_field() {
        let mut s = InputSession::new();
        s.applied(Applied::Char, 5); // no field focused: not counted
        assert_eq!(s.track(Some((4, false))), None);
        assert_eq!(s.track(None), None, "focused, but nothing applied");
    }
}
