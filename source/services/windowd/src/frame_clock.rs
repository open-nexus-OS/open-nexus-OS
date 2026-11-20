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

/// Predictions kept for presents in flight (windowd sends at most one; four kept: one in flight,
/// one judged, slack).
const KEPT: usize = 4;

/// The timeline and the predictions in flight.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct FrameClock {
    shown: Option<Shown>,
    /// `(seq, predicted landing, sent with nothing in flight)` of the latest presents sent.
    predicted: [(u32, u64, bool); KEPT],
    next: usize,
    /// Landings at, after (more than half a period late) and before the time predicted for them,
    /// counted since the start (the telemetry takes differences); an early landing split by
    /// whether nothing (`early0`) or something (`early1`) was in flight when it was sent — the
    /// half-period floor of `next_shown` misses in the first, a double count in the second
    /// (TASK-0343 P7: the split names which before anything is fixed).
    pub(crate) on_time: u32,
    pub(crate) late: u32,
    pub(crate) early: u32,
    pub(crate) early0: u32,
    pub(crate) early1: u32,
}

impl FrameClock {
    pub(crate) const fn new() -> Self {
        Self {
            shown: None,
            predicted: [(0, 0, false); KEPT],
            next: 0,
            on_time: 0,
            late: 0,
            early: 0,
            early0: 0,
            early1: 0,
        }
    }

    /// The latest timing gpud named.
    pub(crate) fn shown(&self) -> Option<Shown> {
        self.shown
    }

    /// When the present windowd sends next is shown, and the step; `None` before a timed ack.
    /// `in_flight + 1` steps after the last landing — every ack re-anchors it — and never earlier
    /// than the first boundary half a period ahead of `now`; only in that floor's case (the
    /// display idled) a step after the newest present still in flight, so a second present after
    /// an idle display is not predicted onto the boundary of the first (TASK-0343 P7). A chain
    /// outside the floor never came back to a landing: one early landing shifted the stream
    /// (board--2026-11-19T17-56-19, 36 `frame pace` lines with `early1 >= 8`).
    pub(crate) fn next_shown(&self, now: u64, in_flight: u32) -> Option<(u64, u32)> {
        let s = self.shown?;
        let (p, step) = (u64::from(s.period_ns), s.step_ns());
        let newest =
            self.predicted.iter().filter(|&&(_, at, _)| at > s.at_ns).map(|&(_, at, _)| at).max();
        let mut at = s.at_ns.saturating_add((u64::from(in_flight) + 1).saturating_mul(step));
        let earliest = now.saturating_add(p / 2);
        if at < earliest {
            let floor = s.at_ns + (earliest - s.at_ns).div_ceil(p) * p;
            at = match newest {
                Some(prev) if in_flight > 0 => floor.max(prev.saturating_add(step)),
                _ => floor,
            };
        }
        Some((at, u32::try_from(step).unwrap_or(u32::MAX)))
    }

    /// windowd sent the present `seq`, predicted to be shown at `at`, with `in_flight` presents
    /// ahead of it.
    pub(crate) fn note_sent(&mut self, seq: u32, at: u64, in_flight: u32) {
        self.predicted[self.next % KEPT] = (seq, at, in_flight == 0);
        self.next = self.next.wrapping_add(1);
    }

    /// gpud acked `seq`, shown as `shown` names: the timeline moves on, and the landing is
    /// judged against the prediction kept for it.
    pub(crate) fn note_shown(&mut self, seq: u32, shown: Shown) {
        self.shown = Some(shown);
        let half = u64::from(shown.period_ns) / 2;
        let Some(k) = self.predicted.iter().position(|&(s, at, _)| s == seq && at != 0) else {
            return;
        };
        let (_, at, idle) = self.predicted[k];
        self.predicted[k] = (0, 0, false);
        if shown.at_ns > at + half {
            self.late = self.late.wrapping_add(1);
        } else if shown.at_ns + half < at {
            self.early = self.early.wrapping_add(1);
            if idle {
                self.early0 = self.early0.wrapping_add(1);
            } else {
                self.early1 = self.early1.wrapping_add(1);
            }
        } else {
            self.on_time = self.on_time.wrapping_add(1);
        }
    }
}

/// Whether a client last pulsed for the frame shown at `pulsed_ns` is owed the pulse of the
/// frame shown at `target_ns` (TASK-0351 P17): a frame on the glass is pulsed once. A client
/// that asks again before the timeline moved on — its tick drew nothing, or it draws faster
/// than the display shows — waits for the next frame; answered at once it asked 2977 times a
/// second on QEMU (`usb-visible--2026-11-18T15-21-35`). A pulse named far ahead of the timeline
/// (the timeline started over) never holds the client back.
pub(crate) fn owes_pulse(pulsed_ns: u64, target_ns: u64, step_ns: u32) -> bool {
    let step = u64::from(step_ns);
    target_ns >= pulsed_ns.saturating_add(step / 2)
        || pulsed_ns > target_ns.saturating_add(8 * step)
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

    /// After an idle display the first present goes on the floor's boundary; the second, sent
    /// while the first is in flight, a step after it — not onto the same boundary (it then
    /// landed "early" behind the first, `early1`).
    #[test]
    fn after_an_idle_display_a_second_present_lands_a_step_after_the_first() {
        let mut c = FrameClock::new();
        let (t0, p) = (1_000 * MS, u64::from(P));
        c.note_shown(1, shown(t0, 1));
        let now = t0 + 10 * p + p / 4;
        let (first, _) = c.next_shown(now, 0).expect("a timeline");
        assert_eq!(first, t0 + 11 * p);
        c.note_sent(2, first, 0);
        let (second, _) = c.next_shown(now + MS, 1).expect("a timeline");
        assert_eq!(second, first + p, "a step after the present in flight");
    }

    /// One early landing does not shift the stream: the present sent behind it is judged early
    /// once (its prediction was made before the landing was known), every later one lands where
    /// predicted — each ack re-anchors the prediction (the chain of the first cut never did).
    #[test]
    fn a_stream_returns_to_its_landings_after_one_early_landing() {
        let mut c = FrameClock::new();
        let (t0, p) = (1_000 * MS, u64::from(P));
        c.note_shown(1, shown(t0, 1));
        let now = t0 + 10 * p + p / 4;
        let (a, _) = c.next_shown(now, 0).expect("a timeline");
        c.note_sent(2, a, 0);
        let (b, _) = c.next_shown(now + MS, 1).expect("a timeline");
        c.note_sent(3, b, 1);
        // Present 2 lands a period early (gpud took it before the floor's guess).
        let mut last = a - p;
        c.note_shown(2, shown(last, 1));
        let (mut flight_seq, mut flight_land) = (3, last + p);
        for seq in 4..14 {
            let (at, _) = c.next_shown(last + 2 * MS, 1).expect("a timeline");
            c.note_sent(seq, at, 1);
            c.note_shown(flight_seq, shown(flight_land, 1));
            last = flight_land;
            (flight_seq, flight_land) = (seq, last + p);
        }
        assert!(c.early1 <= 1, "only the present predicted before the early landing: {}", c.early1);
        assert!(c.on_time >= 9, "the stream lands where predicted: {}", c.on_time);
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
        c.note_sent(2, t0 + p, 0);
        c.note_sent(3, t0 + 2 * p, 1);
        c.note_sent(4, t0 + 3 * p, 2);
        c.note_shown(2, shown(t0 + p, 1));
        c.note_shown(3, shown(t0 + 3 * p, 1));
        c.note_shown(4, shown(t0 + 2 * p, 1));
        assert_eq!((c.on_time, c.late, c.early), (1, 1, 1));
        c.note_shown(2, shown(t0 + 9 * p, 1));
        assert_eq!((c.on_time, c.late, c.early), (1, 1, 1), "an ack is judged once");
    }

    /// An early landing names whether its present was sent with nothing in flight (the
    /// half-period floor's case) or behind another (a landing counted against the wrong slot).
    #[test]
    fn an_early_landing_says_whether_anything_was_in_flight() {
        let mut c = FrameClock::new();
        let (t0, p) = (1_000 * MS, u64::from(P));
        c.note_shown(1, shown(t0, 1));
        c.note_sent(2, t0 + 3 * p, 0);
        c.note_sent(3, t0 + 4 * p, 1);
        c.note_shown(2, shown(t0 + p, 1));
        c.note_shown(3, shown(t0 + 2 * p, 1));
        assert_eq!((c.early, c.early0, c.early1), (2, 1, 1));
        assert_eq!(c.early, c.early0 + c.early1, "the split adds up");
    }

    #[test]
    fn a_frame_is_pulsed_once_and_the_next_when_the_timeline_moved_on() {
        let mut c = FrameClock::new();
        let (t0, p) = (1_000 * MS, u64::from(P));
        c.note_shown(1, shown(t0, 1));
        let (first, step) = c.next_shown(t0 + MS, 0).expect("a timeline");
        assert!(owes_pulse(0, first, step), "never pulsed");
        // Asked again a millisecond later, nothing sent in between: the same frame.
        let (again, _) = c.next_shown(t0 + 2 * MS, 0).expect("a timeline");
        assert!(!owes_pulse(first, again, step));
        // Its frame left (one in flight): the frame after it is owed at once — a client may
        // draw ahead of the display by what the compositor keeps in flight.
        let (ahead, _) = c.next_shown(t0 + 2 * MS, 1).expect("a timeline");
        assert!(owes_pulse(first, ahead, step));
        assert!(!owes_pulse(ahead, ahead, step));
        // A client that took three periods to draw is owed the frame the clock has moved to.
        let (late, _) = c.next_shown(t0 + 3 * p, 0).expect("a timeline");
        assert!(owes_pulse(first, late, step));
        // A landing a little early does not pulse the same frame twice.
        assert!(!owes_pulse(first, first + p / 4, step));
    }

    #[test]
    fn test_reject_holding_a_client_behind_a_pulse_far_ahead_of_the_timeline() {
        let (t0, p) = (1_000 * MS, u64::from(P));
        assert!(!owes_pulse(t0 + 3 * p, t0, P), "ahead by what is in flight: it waits");
        assert!(owes_pulse(t0 + 60 * p, t0, P), "the timeline started over: pulsed");
    }

    #[test]
    fn test_reject_judging_a_landing_without_a_prediction() {
        let mut c = FrameClock::new();
        c.note_shown(7, shown(1_000 * MS, 1));
        assert_eq!((c.on_time, c.late, c.early), (0, 0, 0));
        assert_eq!(c.shown(), Some(shown(1_000 * MS, 1)), "the timeline still moves on");
    }
}
