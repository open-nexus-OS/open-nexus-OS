// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The recorder and the line of a traced pass's wait (`gpud: gpu pass trace (…)`,
//! TASK-0351 P19). The first form kept the first 24 poll iterations: a 41 ms wait polled about
//! once a millisecond, its line passed the console's 256 bytes and was cut, and the board's
//! slowest single-pass frames read nothing (13 of 47 traces readable, board 2026-11-20T10-20-46).
//! Now a step is kept only when what the poll saw changed (the interrupts, the geometry and
//! fragment words), the last poll always, the largest gap between two polls (`gap_us`: ≫ 1 ms
//! means gpud itself did not run) and the pixels the pass's draws cover (`px`, overdraw counted).
//! The line is bounded against the cap by a const-assert; past its steps it prints the first and
//! the last and names how many it left out (`cut=`). Compiled for the host's tests too.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: below (a 41-poll wait, the cap, a wait that changes on every poll)

use core::fmt::Write as _;

/// The console's line cap (`DEBUG_LINE_CAP`).
const CAP: usize = 256;
/// Steps the recorder keeps (changes, plus the last poll in the final slot).
const KEPT: usize = 16;
/// Numbers on the line are printed saturated so the bound below holds for any input.
const US_MAX: u64 = 999_999;
const IRQ_MAX: u32 = 999;
const PX_MAX: u64 = 99_999_999;
/// The `of=` names a trace carries are short case names (`single`, `hover`, the sweep's cases).
const OF_MAX: usize = 16;
/// The line without its steps, every number at its saturated width.
const HEADER_MAX: usize = "gpud: gpu pass trace (of=".len()
    + OF_MAX
    + " iters=999999 irqs=999 late_us=999999 gap_us=999999 px=99999999 cut=99".len()
    + " us_at:irqs:words[g=1,f=2]=".len()
    + ")".len();
/// One step at its widest: ` 999999:999:3`.
const STEP_MAX: usize = " 999999:999:3".len();
/// Steps a line prints: as many as fit under the cap.
const STEPS_PRINTED: usize = (CAP - HEADER_MAX) / STEP_MAX;
const _: () = assert!(HEADER_MAX + STEPS_PRINTED * STEP_MAX <= CAP);
const _: () = assert!(STEPS_PRINTED >= 4 && STEPS_PRINTED <= KEPT);

/// One poll: µs since the wait began, the interrupts seen by then, the words reached (geometry
/// bit 0, fragment bit 1).
pub(crate) type Step = (u32, u32, u8);

/// A traced wait's steps.
pub(crate) struct PassTrace {
    steps: [Step; KEPT],
    kept: usize,
    last: Step,
    polls: u64,
    gap_us: u32,
}

impl PassTrace {
    pub(crate) const fn new() -> Self {
        Self { steps: [(0, 0, 0); KEPT], kept: 0, last: (0, 0, 0), polls: 0, gap_us: 0 }
    }

    /// One poll of the wait.
    pub(crate) fn note(&mut self, step: Step) {
        if self.polls > 0 {
            self.gap_us = self.gap_us.max(step.0.saturating_sub(self.last.0));
        }
        let changed = self.kept == 0 || {
            let k = self.steps[self.kept - 1];
            (k.1, k.2) != (step.1, step.2)
        };
        if changed && self.kept < KEPT {
            self.steps[self.kept] = step;
            self.kept += 1;
        }
        self.last = step;
        self.polls += 1;
    }

    /// The steps to print: the changes, then the last poll unless it is the last change.
    fn printed(&self) -> ([Step; KEPT], usize) {
        let mut all = self.steps;
        let mut n = self.kept;
        if self.polls > 0 && (n == 0 || all[n - 1] != self.last) {
            if n < KEPT {
                all[n] = self.last;
                n += 1;
            } else {
                all[KEPT - 1] = self.last;
            }
        }
        (all, n)
    }

    /// The line, under the console's cap: the first steps and the last, `cut=` those left out.
    pub(crate) fn line(&self, of: &str, irqs: u64, late_us: u64, px: u64) -> alloc::string::String {
        let (steps, n) = self.printed();
        let shown = n.min(STEPS_PRINTED);
        let cut = n - shown;
        let of = of.get(..of.len().min(OF_MAX)).unwrap_or("?");
        let mut s = alloc::string::String::with_capacity(CAP);
        let _ = write!(
            s,
            "gpud: gpu pass trace (of={of} iters={} irqs={} late_us={} gap_us={} px={} cut={} \
             us_at:irqs:words[g=1,f=2]=",
            self.polls.min(US_MAX),
            irqs.min(u64::from(IRQ_MAX)),
            late_us.min(US_MAX),
            u64::from(self.gap_us).min(US_MAX),
            px.min(PX_MAX),
            cut.min(99),
        );
        let first = if cut > 0 { shown - 1 } else { shown };
        let picked = steps[..first].iter().chain(steps[n - (shown - first)..n].iter());
        for &(at, irq, words) in picked {
            let _ = write!(s, " {}:{}:{}", u64::from(at).min(US_MAX), irq.min(IRQ_MAX), words & 3);
        }
        s.push(')');
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_forty_poll_wait_prints_its_changes_and_its_end_within_the_cap() {
        let mut t = PassTrace::new();
        t.note((6, 0, 0));
        t.note((619, 1, 1));
        t.note((644, 2, 1));
        for i in 1..=40u32 {
            t.note((644 + i * 1_021, 2, 1));
        }
        t.note((41_501, 3, 3));
        let line = t.line("single", 3, 31, 2_073_600);
        assert!(line.len() <= CAP, "{} > {CAP}: {line}", line.len());
        assert!(line.ends_with(" 6:0:0 619:1:1 644:2:1 41501:3:3)"), "{line}");
        assert!(
            line.contains("gap_us=1021") && line.contains("px=2073600") && line.contains("cut=0")
        );
    }

    #[test]
    fn test_reject_a_trace_line_past_the_console_cap() {
        let mut t = PassTrace::new();
        // Every poll changes, every number at its widest: the line still fits, naming its cut.
        for i in 0..200u32 {
            t.note((u32::MAX - 200 + i, u32::MAX - i, (i % 4) as u8));
        }
        let line = t.line("a-very-long-case-name-indeed", u64::MAX, u64::MAX, u64::MAX);
        assert!(line.len() <= CAP, "{} > {CAP}: {line}", line.len());
        assert!(!line.contains("cut=0"), "{line}");
    }

    #[test]
    fn a_wait_that_never_polled_prints_no_step() {
        let line = PassTrace::new().line("single", 0, 0, 0);
        assert!(line.ends_with("us_at:irqs:words[g=1,f=2]=)"), "{line}");
    }
}
