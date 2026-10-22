// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: THE SSOT of the UX latency contract (RFC-0108, TASK-0334). The desk board felt like
//! a slideshow and froze while every gate was green (2026-10-20, cycle 47): the gates measured
//! correctness, the numbers existed (`gpu frame us`, the launch stages) but nothing judged them.
//! This crate holds what "instant" and "smooth" mean as numbers — the thresholds human-factors
//! research and the reference platforms converge on — and the ONE verdict rule every
//! measurement line applies: `ok` inside the budget, `warn` past it, `FAIL` past the red line.
//! The services read the budgets here, never their own; the proof ladders read the lines.
//! Budgets are the contract, not the hardware: a 60 Hz panel can never beat a frame of 16.7 ms,
//! and an emulated lane never meets them (its `FAIL` lines are allow-listed by name, QEMU timing
//! is not the contract — the board's is).
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable (the numbers tighten as the packages land; the shape stays)
//! TEST_COVERAGE: the verdict rule and the window on the host (below)

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

/// A frame at 60 Hz: the panel's period. A frame past it is a dropped one (a stutter).
pub const FRAME_US: u64 = 16_667;
/// Dropped frames a 32-frame window tolerates before `warn` (one is a visible hitch).
pub const FRAME_DROPS_WARN: u32 = 1;
/// Dropped frames a 32-frame window tolerates before `FAIL` (a slideshow).
pub const FRAME_DROPS_FAIL: u32 = 4;
/// Frames in a judged window (the gpud window of `gpu frame us`).
pub const FRAME_WINDOW: u32 = 32;

/// An input's way to the pixel: `warn` past this (the reference class delivers a pointer
/// move in 2–3 frames), `FAIL` past [`INPUT_FAIL_US`] (a user feels the lag).
pub const INPUT_WARN_US: u64 = 50_000;
pub const INPUT_FAIL_US: u64 = 100_000;
/// Inputs in a judged window (windowd's `ux input` line).
pub const INPUT_WINDOW: u32 = 16;

/// A tap's visible feedback (the control changes): `FAIL` past this.
pub const TAP_FEEDBACK_US: u64 = 100_000;

/// An app's launch to its first frame, warm (the bundle read, the code shared): `warn` past the
/// target the plan set (TASK-0333 6e: a warm restart under 200 ms), `FAIL` past one second —
/// the point a user stops believing the tap landed.
pub const LAUNCH_WARN_MS: u64 = 300;
pub const LAUNCH_FAIL_MS: u64 = 1_000;

/// The one verdict every measurement line carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Inside the budget.
    Ok,
    /// Past the budget, inside the red line: a hitch a user may notice.
    Warn,
    /// Past the red line: a user notices.
    Fail,
}

impl Verdict {
    /// The word on the line — `FAIL` in capitals so the no-fake-green scan finds it.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Warn => "warn",
            Self::Fail => "FAIL",
        }
    }
}

/// The rule: `value` against a `warn` and a `fail` threshold (`fail >= warn`). Exactly the
/// budget is still inside it.
pub const fn judge(value: u64, warn: u64, fail: u64) -> Verdict {
    if value > fail {
        Verdict::Fail
    } else if value > warn {
        Verdict::Warn
    } else {
        Verdict::Ok
    }
}

/// A launch's first frame against its budget.
pub const fn judge_launch(first_frame_ms: u64) -> Verdict {
    judge(first_frame_ms, LAUNCH_WARN_MS, LAUNCH_FAIL_MS)
}

/// An input's way to the next presented frame against its budget.
pub const fn judge_input(to_present_us: u64) -> Verdict {
    judge(to_present_us, INPUT_WARN_US, INPUT_FAIL_US)
}

/// A window of frames: how many missed the panel's period, and the worst.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FrameWindow {
    pub frames: u32,
    pub dropped: u32,
    pub max_us: u64,
}

impl FrameWindow {
    /// Records one frame's time.
    pub fn note(&mut self, us: u64) {
        self.frames += 1;
        self.dropped += u32::from(us > FRAME_US);
        if us > self.max_us {
            self.max_us = us;
        }
    }

    /// Whether the window is full and due for its line.
    pub const fn full(&self) -> bool {
        self.frames >= FRAME_WINDOW
    }

    /// The window's verdict: the dropped frames against the two bounds.
    pub const fn verdict(&self) -> Verdict {
        judge(self.dropped as u64, FRAME_DROPS_WARN as u64, FRAME_DROPS_FAIL as u64)
    }
}

/// One hop of a chain (RFC-0108 §hops): how long one stage of a frame or an input took — its
/// count, mean and worst over a window. A measurement nobody judges: a chain's ends are judged
/// (`ux input`, `ux frame`), its hops say where the time went. Every service prints its hops in
/// one shape: `<svc>: hops <chain> (<from>><to>=<avg_us>/<max_us>/<count> …)` — each hop with its
/// own count (not every frame passes every hop).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Hop {
    n: u32,
    sum_us: u64,
    max_us: u64,
}

impl Hop {
    pub const fn new() -> Self {
        Self { n: 0, sum_us: 0, max_us: 0 }
    }

    /// Records one pass through the hop.
    pub fn note(&mut self, us: u64) {
        self.n = self.n.saturating_add(1);
        self.sum_us = self.sum_us.saturating_add(us);
        self.max_us = self.max_us.max(us);
    }

    /// The passes recorded in this window.
    pub const fn count(&self) -> u32 {
        self.n
    }

    /// The window's `(count, mean_us, max_us)` — and a fresh window.
    pub fn take(&mut self) -> (u32, u64, u64) {
        let window = (self.n, self.sum_us / u64::from(self.n.max(1)), self.max_us);
        *self = Self::new();
        window
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exactly_the_budget_is_inside_it() {
        assert_eq!(judge(FRAME_US, FRAME_US, FRAME_US * 2), Verdict::Ok);
        assert_eq!(judge(FRAME_US + 1, FRAME_US, FRAME_US * 2), Verdict::Warn);
        assert_eq!(judge(FRAME_US * 2 + 1, FRAME_US, FRAME_US * 2), Verdict::Fail);
        assert_eq!(Verdict::Fail.as_str(), "FAIL");
    }

    #[test]
    fn test_reject_ok_for_a_launch_past_its_budget() {
        assert_eq!(judge_launch(200), Verdict::Ok);
        assert_eq!(judge_launch(300), Verdict::Ok);
        assert_eq!(judge_launch(301), Verdict::Warn);
        assert_eq!(judge_launch(1_000), Verdict::Warn);
        assert_eq!(judge_launch(6_700), Verdict::Fail, "settings on the board, cycle 48");
    }

    #[test]
    fn test_reject_ok_for_an_input_past_its_budget() {
        assert_eq!(judge_input(16_667), Verdict::Ok);
        assert_eq!(judge_input(50_001), Verdict::Warn);
        assert_eq!(judge_input(100_001), Verdict::Fail);
    }

    #[test]
    fn a_window_counts_its_dropped_frames_and_judges_them() {
        let mut w = FrameWindow::default();
        for _ in 0..31 {
            w.note(12_000);
        }
        assert!(!w.full());
        w.note(70_000);
        assert!(w.full());
        assert_eq!((w.dropped, w.max_us), (1, 70_000));
        assert_eq!(w.verdict(), Verdict::Ok, "one drop a window is the warn bound itself");
        w.note(20_000);
        assert_eq!(w.verdict(), Verdict::Warn);
        for _ in 0..3 {
            w.note(20_000);
        }
        assert_eq!(w.verdict(), Verdict::Fail, "cycle 47's slideshow");
    }

    #[test]
    fn a_hop_reports_its_mean_and_worst_and_starts_afresh() {
        let mut h = Hop::new();
        assert_eq!(h.take(), (0, 0, 0), "an empty window divides by nothing");
        h.note(10_000);
        h.note(30_000);
        assert_eq!(h.count(), 2);
        assert_eq!(h.take(), (2, 20_000, 30_000));
        assert_eq!(h.count(), 0);
    }
}
