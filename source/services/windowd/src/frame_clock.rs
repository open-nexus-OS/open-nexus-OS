// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The display's timeline in windowd (RFC-0110, TASK-0343): gpud's present acks name the
//! frame boundary each present reached the glass at, the refresh period and the cadence gpud
//! holds; windowd predicts from them when the present it sends next is shown — the time an
//! animating client's frame pulse names, and the time windowd's own transitions are stepped to —
//! and judges each prediction by the landing gpud reports (`windowd: frame pace (…)`). Board
//! cycle 61: an app stepped its scroll by the moments it happened to run while its frames landed
//! one and two periods apart — the content moved in uneven steps. Pure, host-tested.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: unit tests below
//! INVARIANTS: a prediction lies on the display's boundaries, one step (the cadence's periods)
//!   after the presents in flight and at least half a period ahead of now; before a timed ack
//!   there is none (a client then steps by its own clock); a landing is judged once, against the
//!   prediction kept for its seq.

use nexus_display_proto::frame_time::Shown;

/// Predictions kept for presents in flight (windowd sends at most two).
const KEPT: usize = 4;

/// The timeline and the predictions in flight.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct FrameClock {
    shown: Option<Shown>,
    /// `(seq, predicted landing)` of the latest presents sent.
    predicted: [(u32, u64); KEPT],
    next: usize,
    /// Landings at, after (more than half a period late) and before the time predicted for them,
    /// counted since the start (the telemetry takes differences).
    pub(crate) on_time: u32,
    pub(crate) late: u32,
    pub(crate) early: u32,
}

impl FrameClock {
    pub(crate) const fn new() -> Self {
        Self { shown: None, predicted: [(0, 0); KEPT], next: 0, on_time: 0, late: 0, early: 0 }
    }

    /// The latest timing gpud named.
    pub(crate) fn shown(&self) -> Option<Shown> {
        self.shown
    }

    /// When the present windowd sends next is shown — `in_flight` presents ahead of it, each a
    /// step after the last landing — and the step; `None` before a timed ack. A landing long
    /// past (the display idled) puts it on the first boundary half a period ahead of `now`.
    pub(crate) fn next_shown(&self, now: u64, in_flight: u32) -> Option<(u64, u32)> {
        let s = self.shown?;
        let (p, step) = (u64::from(s.period_ns), s.step_ns());
        let mut at = s.at_ns.saturating_add((u64::from(in_flight) + 1).saturating_mul(step));
        let earliest = now.saturating_add(p / 2);
        if at < earliest {
            at = s.at_ns + (earliest - s.at_ns).div_ceil(p) * p;
        }
        Some((at, u32::try_from(step).unwrap_or(u32::MAX)))
    }

    /// windowd sent the present `seq`, predicted to be shown at `at`.
    pub(crate) fn note_sent(&mut self, seq: u32, at: u64) {
        self.predicted[self.next % KEPT] = (seq, at);
        self.next = self.next.wrapping_add(1);
    }

    /// gpud acked `seq`, shown as `shown` names: the timeline moves on, and the landing is
    /// judged against the prediction kept for it.
    pub(crate) fn note_shown(&mut self, seq: u32, shown: Shown) {
        self.shown = Some(shown);
        let half = u64::from(shown.period_ns) / 2;
        let Some(k) = self.predicted.iter().position(|&(s, at)| s == seq && at != 0) else {
            return;
        };
        let at = self.predicted[k].1;
        self.predicted[k] = (0, 0);
        if shown.at_ns > at + half {
            self.late = self.late.wrapping_add(1);
        } else if shown.at_ns + half < at {
            self.early = self.early.wrapping_add(1);
        } else {
            self.on_time = self.on_time.wrapping_add(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const P: u32 = 16_666_666;
    const MS: u64 = 1_000_000;

    fn shown(at_ns: u64, cadence: u8) -> Shown {
        Shown { at_ns, period_ns: P, cadence }
    }

    #[test]
    fn the_next_present_is_shown_a_step_after_those_in_flight() {
        let mut c = FrameClock::new();
        assert_eq!(c.next_shown(1_000 * MS, 0), None, "no timed ack yet: no prediction");
        let t0 = 1_000 * MS;
        c.note_shown(1, shown(t0, 1));
        let p = u64::from(P);
        // Just after the landing: the next present lands one period on, behind one in flight two.
        assert_eq!(c.next_shown(t0 + MS, 0), Some((t0 + p, P)));
        assert_eq!(c.next_shown(t0 + MS, 1), Some((t0 + 2 * p, P)));
        // At cadence two, two periods a step.
        c.note_shown(2, shown(t0, 2));
        assert_eq!(c.next_shown(t0 + MS, 1), Some((t0 + 4 * p, 2 * P)));
    }

    #[test]
    fn after_an_idle_display_the_prediction_is_the_next_boundary_half_a_period_ahead() {
        let mut c = FrameClock::new();
        let t0 = 1_000 * MS;
        let p = u64::from(P);
        c.note_shown(1, shown(t0, 1));
        let now = t0 + 10 * p + p / 4;
        let (at, _) = c.next_shown(now, 0).expect("a timeline");
        assert_eq!(at, t0 + 11 * p, "on a boundary, half a period ahead of now at least");
        assert_eq!((at - t0) % p, 0);
        assert!(at >= now + p / 2);
    }

    #[test]
    fn each_landing_is_judged_once_against_its_prediction() {
        let mut c = FrameClock::new();
        let t0 = 1_000 * MS;
        let p = u64::from(P);
        c.note_shown(1, shown(t0, 1));
        c.note_sent(2, t0 + p);
        c.note_sent(3, t0 + 2 * p);
        c.note_sent(4, t0 + 3 * p);
        c.note_shown(2, shown(t0 + p, 1));
        c.note_shown(3, shown(t0 + 3 * p, 1));
        c.note_shown(4, shown(t0 + 2 * p, 1));
        assert_eq!((c.on_time, c.late, c.early), (1, 1, 1));
        c.note_shown(2, shown(t0 + 9 * p, 1));
        assert_eq!((c.on_time, c.late, c.early), (1, 1, 1), "an ack is judged once");
    }

    #[test]
    fn test_reject_judging_a_landing_without_a_prediction() {
        let mut c = FrameClock::new();
        c.note_shown(7, shown(1_000 * MS, 1));
        assert_eq!((c.on_time, c.late, c.early), (0, 0, 0));
        assert_eq!(c.shown(), Some(shown(1_000 * MS, 1)), "the timeline still moves on");
    }
}
