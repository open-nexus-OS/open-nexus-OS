// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The compositor's half of the UX latency contract (RFC-0108, TASK-0334): how long an
//! input waits in windowd before the frame that carries it is handed to gpud. The first input
//! staged after a submit starts the clock (newer samples coalesce into the same frame and do
//! not restart it); the next successful submit stops it. Sixteen such inputs make a window,
//! and the window's worst wait is judged against the budget on one line:
//! `windowd: ux input <ok|warn|FAIL> (to_submit_us= of= budget_us=)`. The way from the submit
//! to the pixel is gpud's number (`dc present us`); the way from the device to windowd is
//! hidrawd's (`drain_us`) until the input carries its own stamp (the input contract of the
//! non-parking compositor). Pure and host-tested: no clock of its own.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: host tests below; the line on the board ladder

use nexus_ux_budgets::{judge_input, Verdict, INPUT_WINDOW};

/// The clock from the first pending input to the submit that carries it, windowed.
#[derive(Clone, Copy, Debug, Default)]
pub struct UxInput {
    first_pending_ns: Option<u64>,
    inputs: u32,
    max_us: u64,
}

/// A window's line: its worst wait, its size, the verdict.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UxInputLine {
    pub max_us: u64,
    pub inputs: u32,
    pub verdict: Verdict,
}

impl UxInput {
    /// An input was staged at `now_ns`; the first since the last submit starts the clock.
    pub fn note_input(&mut self, now_ns: u64) {
        if self.first_pending_ns.is_none() {
            self.first_pending_ns = Some(now_ns);
        }
    }

    /// A frame was handed to gpud at `now_ns`: the pending input (if any) arrived. Returns the
    /// window's line once it holds `INPUT_WINDOW` inputs, and starts the next window.
    pub fn note_submit(&mut self, now_ns: u64) -> Option<UxInputLine> {
        let first = self.first_pending_ns.take()?;
        let us = now_ns.saturating_sub(first) / 1000;
        self.inputs += 1;
        if us > self.max_us {
            self.max_us = us;
        }
        if self.inputs < INPUT_WINDOW {
            return None;
        }
        let line = UxInputLine {
            max_us: self.max_us,
            inputs: self.inputs,
            verdict: judge_input(self.max_us),
        };
        self.inputs = 0;
        self.max_us = 0;
        Some(line)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_pending_input_starts_the_clock_and_the_submit_stops_it() {
        let mut ux = UxInput::default();
        assert_eq!(ux.note_submit(5_000_000), None, "a submit without input is not an input");
        ux.note_input(1_000_000);
        ux.note_input(9_000_000); // coalesced into the same frame: the clock stays
        for _ in 0..(INPUT_WINDOW - 1) {
            assert_eq!(ux.note_submit(1_000_000 + 10_000_000), None);
            ux.note_input(1_000_000);
        }
        let line = ux.note_submit(1_000_000 + 10_000_000).expect("the window is full");
        assert_eq!((line.max_us, line.inputs, line.verdict), (10_000, INPUT_WINDOW, Verdict::Ok));
    }

    #[test]
    fn test_reject_ok_when_one_input_waited_past_the_red_line() {
        let mut ux = UxInput::default();
        for i in 0..INPUT_WINDOW {
            ux.note_input(0);
            let late = if i == 3 { 120_000_000 } else { 8_000_000 };
            if let Some(line) = ux.note_submit(late) {
                assert_eq!((line.max_us, line.verdict), (120_000, Verdict::Fail));
                return;
            }
        }
        panic!("the window never closed");
    }
}
