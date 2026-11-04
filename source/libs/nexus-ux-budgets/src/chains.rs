// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The motion chain's hop in an app (RFC-0108 §hops, TASK-0354 P1): what a Tier-2 motion
//! costs the app while it runs. A frame pulse that finds motion live starts the hop; the present
//! (or motion frame) that answers it ends the hop; every pulse of the episode is a wake-up
//! (`ticks`). The episode ends when a pulse finds no motion live (closed by that pulse's own
//! answer, so its frame counts), and the app says one line:
//! `apphost: hops motion (pulse>sent=<avg_us>/<max_us>/<count> ticks=<wake-ups>)` — times and
//! counts only, no verdict (the frame's verdict is gpud's `ux frame`). Pure: no clock of its own,
//! fixed storage.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: the unit tests below (an episode's counts, `test_reject_*` for a pulse without
//!   motion and an answer without a pulse); the line on the board

use crate::Hop;

/// One Tier-2 motion episode's hop and wake-up count.
#[derive(Clone, Copy, Debug, Default)]
pub struct MotionHops {
    /// The pulse that found motion live and is not answered yet.
    pulse_rx_ns: Option<u64>,
    hop: Hop,
    /// Wake-ups of the open episode (pulses that found motion live).
    ticks: u32,
    episode: bool,
    /// The episode fell silent while its last pulse is still unanswered: the answer closes it.
    closing: bool,
}

/// An episode's line: `(count, mean_us, max_us, ticks)`.
pub type EpisodeLine = (u32, u64, u64, u32);

impl MotionHops {
    #[must_use]
    pub const fn new() -> Self {
        Self { pulse_rx_ns: None, hop: Hop::new(), ticks: 0, episode: false, closing: false }
    }

    /// A frame pulse arrived at `rx_ns`; `live` says whether any motion is running. A pulse that
    /// finds motion live is a wake-up and, when none is pending, starts the hop.
    pub fn pulse(&mut self, rx_ns: u64, live: bool) {
        if !live {
            return;
        }
        self.episode = true;
        self.closing = false;
        self.ticks = self.ticks.saturating_add(1);
        if self.pulse_rx_ns.is_none() {
            self.pulse_rx_ns = Some(rx_ns);
        }
    }

    /// A present or a motion frame left at `now_ns`: the pending pulse's hop ends — and an
    /// episode that fell silent on that pulse closes with it, yielding its line.
    pub fn answered(&mut self, now_ns: u64) -> Option<EpisodeLine> {
        let rx = self.pulse_rx_ns.take()?;
        self.hop.note(now_ns.saturating_sub(rx) / 1000);
        self.closing.then(|| self.close())
    }

    /// After a pulse's work: `live` says whether motion still runs. An open episode that fell
    /// silent closes and yields its line — at once when nothing is pending, else with the
    /// answer to the last pulse, so the frame that pulse owes is counted.
    pub fn settled(&mut self, live: bool) -> Option<EpisodeLine> {
        if live || !self.episode {
            return None;
        }
        if self.pulse_rx_ns.is_some() {
            self.closing = true;
            return None;
        }
        Some(self.close())
    }

    fn close(&mut self) -> EpisodeLine {
        self.episode = false;
        self.closing = false;
        self.pulse_rx_ns = None;
        let (n, avg, max) = self.hop.take();
        (n, avg, max, core::mem::take(&mut self.ticks))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// TASK-0354 P1: three pulses with motion live, two answered; the episode closes once a pulse
    /// finds nothing live and says 2 hops of 3 wake-ups.
    #[test]
    fn an_episode_counts_its_wake_ups_and_its_answered_pulses() {
        let mut m = MotionHops::new();
        assert_eq!(m.settled(false), None, "no episode without a live pulse");
        m.pulse(1_000_000, true);
        assert_eq!(m.answered(5_000_000), None, "the episode is open: no line yet");
        m.pulse(9_000_000, true);
        m.pulse(10_000_000, true); // the second pulse is still pending: joins it
        assert_eq!(m.answered(17_000_000), None);
        assert_eq!(m.settled(true), None, "still live: the episode stays open");
        assert_eq!(m.settled(false), Some((2, 6_000, 8_000, 3)));
        assert_eq!(m.settled(false), None, "closed once");
    }

    /// The last pulse of an episode is the one that finds the motion settled: its own frame is
    /// still owed, so the line waits for that frame's answer and counts its hop.
    #[test]
    fn the_last_pulse_s_frame_closes_the_episode() {
        let mut m = MotionHops::new();
        m.pulse(1_000_000, true);
        assert_eq!(m.settled(false), None, "pending: the answer closes it");
        assert_eq!(m.answered(4_000_000), Some((1, 3_000, 3_000, 1)));
        assert_eq!(m.answered(5_000_000), None, "nothing pending after the close");
        m.pulse(6_000_000, true);
        assert_eq!(m.settled(false), None);
        m.pulse(7_000_000, true); // motion resumed before the answer: the episode goes on
        assert_eq!(m.answered(8_000_000), None);
        assert_eq!(m.settled(false), Some((1, 2_000, 2_000, 2)));
    }

    /// A pulse without motion live is no wake-up of a motion and starts nothing; an answer with
    /// no pending pulse notes nothing.
    #[test]
    fn test_reject_a_pulse_without_motion_and_an_answer_without_a_pulse() {
        let mut m = MotionHops::new();
        m.pulse(1_000_000, false);
        assert_eq!(m.answered(2_000_000), None);
        assert_eq!(m.settled(false), None);
    }
}
