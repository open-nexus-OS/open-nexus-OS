// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The cadence gpud holds on the board's display (RFC-0110, TASK-0343): a present lands
//! every `cadence` refresh periods while frames come — one while the frames' draws fit a period,
//! two or three while they need more — and a frame drawn before its slot is held until the
//! boundary before it, so the frames on the glass are evenly apart and a motion computed for the
//! time a frame is shown is shown at that time. Board cycle 61: frames of 25 ms on a 16.7 ms
//! panel landed one and two periods apart in turn, the content moving in uneven steps (judder).
//! Each landing is timed at the frame boundary its flip was taken at — read from the scan line
//! counter, not from the wake that found it — and that time goes back with the present's ack.
//! Compiled for the host's tests too (the ring's pattern, `flip_ring.rs`).
//! OWNERS: @gpu @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the rules below (host); on the board `gpud: dc flips (… cadence= paced= late=)`
//! INVARIANTS: the cadence follows the second longest of the last `WINDOW` draws (one slow frame
//!   keeps the pace, two change it); a frame is never held past its slot; at cadence one nothing
//!   is held.

use nexus_gfx::backend::dc::Mode;

/// Draws the cadence is decided from.
pub(super) const WINDOW: usize = 8;
/// The longest cadence: a present every three periods (20 Hz on a 60 Hz panel).
pub(super) const CADENCE_MAX: u8 = 3;
/// What a draw must leave of its slot: a latch written this close to the boundary may miss it.
pub(super) const MARGIN_NS: u64 = 2_000_000;

/// The scan's clock: the refresh period and the time of one scan line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Timing {
    pub(super) period_ns: u64,
    pub(super) line_ns: u64,
}

impl Timing {
    /// The clock of `mode`'s timing (its totals at its pixel clock); 60 Hz when it names none.
    pub(super) fn of(mode: &Mode) -> Self {
        let (lines, khz) = (u64::from(mode.v_total()), u64::from(mode.pixel_clock_khz));
        let frame_px = u64::from(mode.h_total()) * lines;
        if lines == 0 || khz == 0 || frame_px == 0 {
            return Self { period_ns: 16_666_666, line_ns: 16_666_666 / 1125 };
        }
        let period_ns = frame_px * 1_000_000 / khz;
        Self { period_ns, line_ns: period_ns / lines }
    }

    /// The boundary the frame being scanned began at: `line` lines before `now`.
    pub(super) fn boundary(&self, now: u64, line: u32) -> u64 {
        now.saturating_sub(u64::from(line) * self.line_ns)
    }
}

/// The cadence and the landings it is held against.
pub(super) struct Pace {
    timing: Timing,
    draws: [u64; WINDOW],
    noted: usize,
    cadence: u8,
    /// The boundary the last flip was taken at (0: none yet).
    last_shown_ns: u64,
    /// Landings later than their slot.
    pub(super) late: u32,
}

impl Pace {
    pub(super) const fn new(timing: Timing) -> Self {
        Self { timing, draws: [0; WINDOW], noted: 0, cadence: 1, last_shown_ns: 0, late: 0 }
    }

    pub(super) fn timing(&self) -> Timing {
        self.timing
    }

    pub(super) fn cadence(&self) -> u8 {
        self.cadence
    }

    /// A present's draw took `ns` (received → drawn): the cadence is the fewest periods the
    /// window's second longest draw fits with the margin.
    pub(super) fn note_draw(&mut self, ns: u64) {
        self.draws[self.noted % WINDOW] = ns;
        self.noted += 1;
        let filled = self.noted.min(WINDOW);
        let (mut first, mut second) = (0u64, 0u64);
        for &d in &self.draws[..filled] {
            if d > first {
                second = first;
                first = d;
            } else if d > second {
                second = d;
            }
        }
        let key = if filled >= 2 { second } else { first };
        let p = self.timing.period_ns;
        self.cadence =
            (1..=CADENCE_MAX).find(|&c| key + MARGIN_NS <= u64::from(c) * p).unwrap_or(CADENCE_MAX);
    }

    /// The boundary a flip latched at `latched_ns` was taken at, the scan's present frame having
    /// begun at `boundary_ns`: the first boundary after the latch was written (a landing found a
    /// frame or more late is timed where it happened).
    pub(super) fn taken_at(&self, latched_ns: u64, boundary_ns: u64) -> u64 {
        let p = self.timing.period_ns.max(1);
        match boundary_ns.checked_sub(latched_ns) {
            Some(d) if d > 0 => boundary_ns - (d - 1) / p * p,
            _ => boundary_ns,
        }
    }

    /// A flip landed at the boundary `at_ns`.
    pub(super) fn note_shown(&mut self, at_ns: u64) {
        let p = self.timing.period_ns;
        if self.last_shown_ns != 0 && at_ns > self.slot() + p / 2 {
            self.late = self.late.saturating_add(1);
        }
        self.last_shown_ns = at_ns;
    }

    /// The boundary the next flip is due at: the last one's plus `cadence` periods.
    fn slot(&self) -> u64 {
        self.last_shown_ns + u64::from(self.cadence) * self.timing.period_ns
    }

    /// Whether a drawn frame may be latched now, the scan's present frame having begun at
    /// `boundary_ns`: it is taken at the next boundary, which must not come before its slot
    /// (half a period of slack for the clock's reading). At cadence one, or before a first
    /// landing, always.
    pub(super) fn may_latch(&self, boundary_ns: u64) -> bool {
        if self.cadence <= 1 || self.last_shown_ns == 0 {
            return true;
        }
        let p = self.timing.period_ns;
        boundary_ns + p + p / 2 >= self.slot()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const P: u64 = 16_666_666;
    const MS: u64 = 1_000_000;

    fn sixty() -> Pace {
        Pace::new(Timing { period_ns: P, line_ns: P / 1125 })
    }

    #[test]
    fn the_scan_clock_is_the_mode_s_totals_at_its_pixel_clock() {
        let mode = nexus_gfx::backend::dc::cea_mode(16).expect("1080p60");
        let t = Timing::of(&mode);
        assert_eq!(t, Timing { period_ns: 16_666_666, line_ns: 14_814 });
        assert_eq!(Pace::new(t).timing(), t);
        // Line 37 of a frame, 0.55 ms after its boundary.
        assert_eq!(t.boundary(1_000_000_000, 37), 1_000_000_000 - 37 * 14_814);
    }

    #[test]
    fn the_cadence_follows_the_second_longest_draw_of_the_window() {
        let mut pace = sixty();
        for _ in 0..WINDOW {
            pace.note_draw(10 * MS);
        }
        assert_eq!(pace.cadence(), 1);
        pace.note_draw(25 * MS);
        assert_eq!(pace.cadence(), 1, "one slow frame keeps the pace");
        pace.note_draw(25 * MS);
        assert_eq!(pace.cadence(), 2, "two change it");
        for _ in 0..WINDOW {
            pace.note_draw(40 * MS);
        }
        assert_eq!(pace.cadence(), 3);
        for _ in 0..WINDOW {
            pace.note_draw(90 * MS);
        }
        assert_eq!(pace.cadence(), CADENCE_MAX, "never slower than the longest cadence");
        for _ in 0..WINDOW {
            pace.note_draw(14 * MS);
        }
        assert_eq!(pace.cadence(), 1, "the margin still fits one period");
        for _ in 0..WINDOW {
            pace.note_draw(15 * MS);
        }
        assert_eq!(pace.cadence(), 2, "a draw closer than the margin to the boundary does not");
    }

    #[test]
    fn a_frame_drawn_before_its_slot_is_held_to_the_boundary_before_it() {
        let mut pace = sixty();
        for _ in 0..WINDOW {
            pace.note_draw(25 * MS);
        }
        assert_eq!(pace.cadence(), 2);
        let b0 = 1_000 * MS;
        assert!(pace.may_latch(b0), "before a first landing nothing is held");
        pace.note_shown(b0);
        // Drawn in the frame that began at the landing: the next boundary is a period early.
        assert!(!pace.may_latch(b0));
        // In the next frame: taken at the boundary after it — the slot.
        assert!(pace.may_latch(b0 + P));
        // Late: never held past the slot.
        assert!(pace.may_latch(b0 + 3 * P));
    }

    #[test]
    fn test_reject_a_hold_at_cadence_one() {
        let mut pace = sixty();
        for _ in 0..WINDOW {
            pace.note_draw(10 * MS);
        }
        pace.note_shown(1_000 * MS);
        assert!(pace.may_latch(1_000 * MS), "at cadence one a drawn frame is latched at once");
    }

    #[test]
    fn a_landing_is_timed_at_the_boundary_its_flip_was_taken_at() {
        let pace = sixty();
        let b0 = 1_000 * MS;
        // Latched mid-frame, found at the wake of the next boundary.
        assert_eq!(pace.taken_at(b0 + 5 * MS, b0 + P), b0 + P);
        // Found two frames later by the loop: still the first boundary after the latch.
        assert_eq!(pace.taken_at(b0 + 5 * MS, b0 + 3 * P), b0 + P);
        // Latched exactly on a boundary: taken at the next one.
        assert_eq!(pace.taken_at(b0, b0 + P), b0 + P);
    }

    #[test]
    fn a_landing_past_its_slot_is_counted_late() {
        let mut pace = sixty();
        pace.note_shown(1_000 * MS);
        pace.note_shown(1_000 * MS + P);
        assert_eq!(pace.late, 0);
        pace.note_shown(1_000 * MS + 3 * P);
        assert_eq!(pace.late, 1, "a period skipped at cadence one");
    }
}
