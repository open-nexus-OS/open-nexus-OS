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

/// One Tier-2 motion episode's hop and wake-up count — and what its frames cost the app
/// (TASK-0354 P1, sharpened for TASK-0351 P15): the raster of each frame, the rows it painted,
/// the damage it declared and the motion tokens that ran.
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
    /// The CPU raster of each frame of the episode (µs), and the rows it painted.
    paint: Hop,
    rows: u64,
    /// The damage the episode's presents declared (pixels) and how many declared it.
    px: u64,
    presents: u32,
    /// The motion tokens that started during the episode (bit = the token's id).
    tokens: u32,
}

/// An episode's line.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Episode {
    /// Answered pulses, and the pulse → sent hop's mean and max (µs).
    pub frames: u32,
    pub mean_us: u64,
    pub max_us: u64,
    /// Wake-ups (pulses that found motion live).
    pub ticks: u32,
    /// The raster per frame: mean and max (µs), and the rows painted per raster.
    pub paint_mean_us: u64,
    pub paint_max_us: u64,
    pub rows: u64,
    /// The damage declared per present (pixels).
    pub px: u64,
    /// The tokens that started (bit = id).
    pub tokens: u32,
}

impl MotionHops {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            pulse_rx_ns: None,
            hop: Hop::new(),
            ticks: 0,
            episode: false,
            closing: false,
            paint: Hop::new(),
            rows: 0,
            px: 0,
            presents: 0,
            tokens: 0,
        }
    }

    /// A raster of `rows` rows took `us` — counted while an episode is open.
    pub fn painted(&mut self, us: u64, rows: u32) {
        if self.episode {
            self.paint.note(us);
            self.rows = self.rows.saturating_add(u64::from(rows));
        }
    }

    /// A present declared `px` pixels of damage — counted while an episode is open.
    pub fn damaged(&mut self, px: u64) {
        if self.episode {
            self.px = self.px.saturating_add(px);
            self.presents = self.presents.saturating_add(1);
        }
    }

    /// Motion tokens started since the last pulse (bit = id) — kept while an episode is open.
    pub fn tokens(&mut self, mask: u32) {
        if self.episode {
            self.tokens |= mask;
        }
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
    pub fn answered(&mut self, now_ns: u64) -> Option<Episode> {
        let rx = self.pulse_rx_ns.take()?;
        self.hop.note(now_ns.saturating_sub(rx) / 1000);
        self.closing.then(|| self.close())
    }

    /// After a pulse's work: `live` says whether motion still runs. An open episode that fell
    /// silent closes and yields its line — at once when nothing is pending, else with the
    /// answer to the last pulse, so the frame that pulse owes is counted.
    pub fn settled(&mut self, live: bool) -> Option<Episode> {
        if live || !self.episode {
            return None;
        }
        if self.pulse_rx_ns.is_some() {
            self.closing = true;
            return None;
        }
        Some(self.close())
    }

    fn close(&mut self) -> Episode {
        self.episode = false;
        self.closing = false;
        self.pulse_rx_ns = None;
        let (frames, mean_us, max_us) = self.hop.take();
        let paints = u64::from(self.paint.count().max(1));
        let (_, paint_mean_us, paint_max_us) = self.paint.take();
        Episode {
            frames,
            mean_us,
            max_us,
            ticks: core::mem::take(&mut self.ticks),
            paint_mean_us,
            paint_max_us,
            rows: core::mem::take(&mut self.rows) / paints,
            px: core::mem::take(&mut self.px)
                / u64::from(core::mem::take(&mut self.presents).max(1)),
            tokens: core::mem::take(&mut self.tokens),
        }
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
        let line =
            Episode { frames: 2, mean_us: 6_000, max_us: 8_000, ticks: 3, ..Episode::default() };
        assert_eq!(m.settled(false), Some(line));
        assert_eq!(m.settled(false), None, "closed once");
    }

    /// TASK-0351 P15: what the episode's frames cost — the raster per frame and its rows, the
    /// damage per present, the tokens that ran — all per episode, nothing outside one.
    #[test]
    fn an_episode_says_what_its_frames_cost() {
        let mut m = MotionHops::new();
        m.painted(9_000, 600);
        m.damaged(1_000_000);
        m.tokens(1 << 8);
        m.pulse(1_000_000, true);
        m.painted(3_000, 600);
        m.painted(5_000, 400);
        m.damaged(200_000);
        m.damaged(400_000);
        m.tokens(1 << 8);
        m.tokens(1 << 3);
        assert_eq!(m.answered(5_000_000), None);
        let line = m.settled(false).expect("the episode's line");
        assert_eq!((line.frames, line.mean_us, line.ticks), (1, 4_000, 1));
        assert_eq!((line.paint_mean_us, line.paint_max_us, line.rows), (4_000, 5_000, 500));
        assert_eq!((line.px, line.tokens), (300_000, (1 << 8) | (1 << 3)));
        assert_eq!(m.settled(false), None, "closed once");
        m.pulse(6_000_000, true);
        assert_eq!(m.answered(7_000_000), None);
        let next = m.settled(false).expect("a fresh episode");
        assert_eq!((next.paint_mean_us, next.rows, next.px, next.tokens), (0, 0, 0, 0));
    }

    /// The last pulse of an episode is the one that finds the motion settled: its own frame is
    /// still owed, so the line waits for that frame's answer and counts its hop.
    #[test]
    fn the_last_pulse_s_frame_closes_the_episode() {
        let mut m = MotionHops::new();
        m.pulse(1_000_000, true);
        assert_eq!(m.settled(false), None, "pending: the answer closes it");
        let one =
            Episode { frames: 1, mean_us: 3_000, max_us: 3_000, ticks: 1, ..Episode::default() };
        assert_eq!(m.answered(4_000_000), Some(one));
        assert_eq!(m.answered(5_000_000), None, "nothing pending after the close");
        m.pulse(6_000_000, true);
        assert_eq!(m.settled(false), None);
        m.pulse(7_000_000, true); // motion resumed before the answer: the episode goes on
        assert_eq!(m.answered(8_000_000), None);
        let two =
            Episode { frames: 1, mean_us: 2_000, max_us: 2_000, ticks: 2, ..Episode::default() };
        assert_eq!(m.settled(false), Some(two));
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
