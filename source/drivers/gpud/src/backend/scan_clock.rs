// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The scan clock of a display without a vertical blank (TASK-0351 P17, RFC-0101 §8).
//! QEMU's virtio GPU completes a present in microseconds; acked at once, every completion
//! started the compositor's next frame and a window's open transition ran at up to 2083
//! presents a second, on the cores the opening app needed (`usb-visible--2026-11-18T14-18-19`).
//! A present on such a display reaches the glass at a frame boundary the driver keeps itself:
//! one refresh period after the frame before it. A present whose boundary has already come is
//! acked at once, as before; one that came sooner is held and acked at its boundary, when the
//! loop's one-shot timer wakes it — the same deferred ack a display with a flip interrupt
//! gives (`Display::present_ack_deferred`, `Display::vblank_wake`). Pure: the loop gives it
//! the time.
//! OWNERS: @gpu @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `mod tests` below (host); QEMU `usb-visible` (`windowd: loop hz` through a
//!   window's open transition)
//! INVARIANTS: held acks leave in the order their presents came, and no ack leaves at once
//!   while one is held (short of `HELD_MAX` held, twice what the compositor keeps in flight);
//!   two frames are never less than a period apart; without a timer nothing is held.

/// Presents held at once: the compositor keeps two in flight.
const HELD_MAX: usize = 4;

/// A present waiting for its frame boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Held {
    pub seq: u32,
    pub status: u8,
    /// The boundary it reaches the glass at.
    pub at_ns: u64,
}

/// The frame boundaries of a display that has none of its own.
#[derive(Default)]
pub(crate) struct ScanClock {
    /// The loop has a timer that wakes it at a boundary. Without one nothing is held.
    pub(crate) timed: bool,
    /// The last frame's boundary.
    last_ns: u64,
    held: [Option<Held>; HELD_MAX],
    len: usize,
}

impl ScanClock {
    /// A present completed at `now_ns` on a display of refresh `period_ns`. `true`: it is held
    /// until its boundary ([`ScanClock::next_ns`], [`ScanClock::landed`]). `false`: its
    /// boundary is now — the loop acks it at once.
    pub(crate) fn hold(&mut self, seq: u32, status: u8, now_ns: u64, period_ns: u64) -> bool {
        let boundary = self.last_ns.saturating_add(period_ns);
        // Its boundary has come and nothing waits before it — or there is no way to wait.
        if !self.timed || self.len == HELD_MAX || (self.len == 0 && boundary <= now_ns) {
            // Presents ahead of it leave first: an ack never overtakes an earlier one.
            self.last_ns = self.last_ns.max(now_ns);
            return false;
        }
        let at_ns = boundary.max(now_ns);
        self.held[self.len] = Some(Held { seq, status, at_ns });
        self.len += 1;
        self.last_ns = at_ns;
        true
    }

    /// The boundary the timer is armed for: the oldest held present's.
    pub(crate) fn next_ns(&self) -> Option<u64> {
        self.held[0].map(|h| h.at_ns)
    }

    /// The oldest held present, once its boundary has come.
    pub(crate) fn landed(&mut self, now_ns: u64) -> Option<Held> {
        let first = self.held[0].filter(|h| h.at_ns <= now_ns)?;
        self.held.copy_within(1.., 0);
        self.held[HELD_MAX - 1] = None;
        self.len -= 1;
        Some(first)
    }

    /// Whether a held present waits.
    #[cfg(test)]
    fn holds(&self) -> bool {
        self.len != 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PERIOD: u64 = 16_666_667;

    fn timed() -> ScanClock {
        ScanClock { timed: true, ..ScanClock::default() }
    }

    #[test]
    fn a_present_after_a_rest_is_acked_at_once_and_the_next_waits_a_period() {
        let mut c = timed();
        let t0 = 5_000_000_000;
        assert!(!c.hold(1, 0, t0, PERIOD), "nothing came before it");
        assert_eq!(c.next_ns(), None);
        // The next completes 200 µs later: it is the next frame, a period on.
        assert!(c.hold(2, 0, t0 + 200_000, PERIOD));
        assert_eq!(c.next_ns(), Some(t0 + PERIOD));
        assert_eq!(c.landed(t0 + PERIOD - 1), None);
        assert_eq!(c.landed(t0 + PERIOD), Some(Held { seq: 2, status: 0, at_ns: t0 + PERIOD }));
        assert!(!c.holds());
        // One that completes after its boundary is acked at once again.
        assert!(!c.hold(3, 0, t0 + 3 * PERIOD, PERIOD));
    }

    #[test]
    fn frames_in_a_run_are_a_period_apart_and_leave_in_order() {
        let mut c = timed();
        let t0 = 1_000_000_000;
        assert!(!c.hold(1, 0, t0, PERIOD));
        // Two in flight, both done within a millisecond.
        assert!(c.hold(2, 0, t0 + 300_000, PERIOD));
        assert!(c.hold(3, 3, t0 + 900_000, PERIOD));
        assert_eq!(c.next_ns(), Some(t0 + PERIOD));
        // Both boundaries have come by the time the loop looks: the older one first.
        let now = t0 + 2 * PERIOD + 5;
        assert_eq!(c.landed(now).map(|h| (h.seq, h.at_ns)), Some((2, t0 + PERIOD)));
        assert_eq!(c.next_ns(), Some(t0 + 2 * PERIOD));
        assert_eq!(c.landed(now).map(|h| (h.seq, h.status)), Some((3, 3)));
        assert_eq!(c.landed(now), None);
        // A thousand completions a second leave as sixty frames.
        let mut c = timed();
        let mut acked = 0u32;
        for seq in 0..1000u32 {
            let now = t0 + u64::from(seq) * 1_000_000;
            while c.landed(now).is_some() {
                acked += 1;
            }
            // The compositor keeps two in flight: it sends when one is acked.
            if !c.holds() && !c.hold(seq, 0, now, PERIOD) {
                acked += 1;
            }
        }
        assert!((59..=61).contains(&acked), "{acked} frames in a second");
    }

    #[test]
    fn an_ack_never_overtakes_a_held_one_and_nothing_is_held_without_a_timer() {
        let mut c = timed();
        let t0 = 1_000_000_000;
        assert!(!c.hold(1, 0, t0, PERIOD));
        assert!(c.hold(2, 0, t0 + 1, PERIOD));
        // The loop was away for three periods; the next present's own boundary has come, but
        // present 2 has not left yet — it waits behind it.
        let late = t0 + 3 * PERIOD;
        assert!(c.hold(3, 0, late, PERIOD));
        assert_eq!(c.landed(late).map(|h| h.seq), Some(2));
        assert_eq!(c.landed(late).map(|h| h.seq), Some(3));
        // More in flight than the clock holds: acked at once rather than lost.
        let mut c = timed();
        assert!(!c.hold(0, 0, t0, PERIOD));
        for seq in 1..=HELD_MAX as u32 {
            assert!(c.hold(seq, 0, t0 + 1, PERIOD));
        }
        assert!(!c.hold(9, 0, t0 + 2, PERIOD));
        // No timer: the loop could not wake for a boundary.
        let mut c = ScanClock::default();
        assert!(!c.hold(1, 0, t0, PERIOD));
        assert!(!c.hold(2, 0, t0 + 1, PERIOD));
        assert_eq!(c.next_ns(), None);
    }
}
