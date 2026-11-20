// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: one frame's timeline (RFC-0108 §hops, amended 2026-11-19, TASK-0373): the window
//! averages (`<svc>: hops <chain> (…)`) say THAT time went, not in which frame, at which hop or
//! why — a regression was read from sums and two notes of the same day were wrong. Every process
//! of the frame chain (app-host, windowd, gpud) takes a [`Timeline`] per frame into a [`Ring`]:
//! its stamps (all `nsec()`, one kernel clock), its waits, yields and bytes copied, a few counts
//! of its own. A frame its process judges slow by the rules below is printed ONCE, when the
//! process is idle — `<svc>: hops one (…)` — and a [`Hist`] per window says how the hops
//! spread; `nx hops` joins the three processes' lines into one timeline per frame by the ids
//! already on the wire. A record costs no allocation and no print on the frame path; lines are
//! cut at [`LINE_CAP`] and name only the fields of [`VOCABULARY`]. Times and counts only: never
//! what an input was.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `mod tests` below

use core::fmt::Write as _;

/// Stamps a record holds (`stamps[0]` is the frame's first, `t0`).
pub const STAMPS: usize = 8;
/// Service-specific counts a record holds (rows and px, layers and rects, passes, draws …).
pub const COUNTS: usize = 8;
/// Frames a process's ring keeps.
pub const RING: usize = 32;
/// A hop line is cut at the console's line cap.
pub const LINE_CAP: usize = 256;
/// At most this many `hops one` lines a window and process.
pub const ONE_PER_WINDOW: u32 = 8;
/// The histogram's bucket bounds (µs): below 1, 2, 4, 8, 16.7, 33.3, 66.7 ms, and above.
pub const BUCKETS_US: [u64; 7] = [1_000, 2_000, 4_000, 8_000, 16_667, 33_333, 66_667];
/// gpud halves its cadence past this draw time (`frame_pace`): a frame's work above it is slow.
pub const CADENCE_DRAW_US: u64 = 14_667;
/// A hop is printed saturated at this many µs (1 s — a longer hop is a stall the line names as
/// such): six digits, so the worst gpud line stays under [`LINE_CAP`].
pub const HOP_PRINT_MAX_US: u64 = 999_999;

/// A hop's µs as printed: saturated at [`HOP_PRINT_MAX_US`].
pub const fn hop_print(us: u64) -> u64 {
    if us > HOP_PRINT_MAX_US {
        HOP_PRINT_MAX_US
    } else {
        us
    }
}
/// The field names a hop line may carry (`<svc>: hops one|hist (…)`): the decoder's contract.
#[rustfmt::skip]
pub const VOCABULARY: [&str; 36] = [
    "s", "f", "from", "t0", "in>disp", "disp>layout", "layout>raster", "raster>sent", "rx>owed",
    "owed>scene", "scene>sent", "sent>acked", "shown", "rx>plan", "plan>emit", "emit>sub",
    "sub>gpu", "gpu>latch", "latch>shown", "w", "y", "cp", "rows", "px", "layers", "rects",
    "passes", "draws", "gw", "whole", "wpx", "by", "slot_px", "scene_px", "one", "lost",
];

/// One frame in one process.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Timeline {
    /// The frame's id in this process (windowd's and gpud's present seq; app-host's `seq`).
    pub id: u32,
    /// app-host: the surface the seq belongs to; elsewhere 0.
    pub aux: u32,
    /// The stamps (ns), `0` = not taken; `stamps[0]` is `t0`.
    pub stamps: [u64; STAMPS],
    /// Blocking calls on this frame's path and the time in them (µs).
    pub waits: u32,
    pub wait_us: u64,
    /// `yield_()` calls on this frame's path.
    pub yields: u32,
    /// Bytes copied for this frame.
    pub copied: u64,
    /// The process's own counts (named by its line).
    pub counts: [u64; COUNTS],
    /// Judged slow by its process's rule; printed once.
    pub slow: bool,
    printed: bool,
}

impl Timeline {
    /// Stamp `k` at `ns` (an out-of-range `k` is ignored).
    pub fn stamp(&mut self, k: usize, ns: u64) {
        if let Some(s) = self.stamps.get_mut(k) {
            *s = ns;
        }
    }

    /// The time from stamp `a` to stamp `b` (µs), when both were taken in that order.
    pub fn hop_us(&self, a: usize, b: usize) -> Option<u64> {
        let (x, y) = (*self.stamps.get(a)?, *self.stamps.get(b)?);
        (x != 0 && y >= x).then(|| (y - x) / 1000)
    }

    /// The frame's span in this process (µs): from `t0` to its last stamp.
    pub fn span_us(&self) -> u64 {
        let last = self.stamps.iter().copied().filter(|&s| s != 0).max().unwrap_or(0);
        last.saturating_sub(self.stamps[0]) / 1000
    }

    /// A wait on this frame's path of `us` µs.
    pub fn note_wait(&mut self, us: u64) {
        self.waits = self.waits.saturating_add(1);
        self.wait_us = self.wait_us.saturating_add(us);
    }
}

/// A process's last [`RING`] frames.
#[derive(Clone, Copy, Debug)]
pub struct Ring {
    recs: [Timeline; RING],
    next: usize,
    /// Slow frames overwritten before they were printed.
    lost: u32,
}

impl Default for Ring {
    fn default() -> Self {
        Self::new()
    }
}

impl Ring {
    pub const fn new() -> Self {
        const EMPTY: Timeline = Timeline {
            id: 0,
            aux: 0,
            stamps: [0; STAMPS],
            waits: 0,
            wait_us: 0,
            yields: 0,
            copied: 0,
            counts: [0; COUNTS],
            slow: false,
            printed: false,
        };
        Self { recs: [EMPTY; RING], next: 0, lost: 0 }
    }

    /// A new frame `(id, aux)` starting at `t0_ns`: the oldest record gives way (counted
    /// `lost` when it was slow and not printed yet).
    pub fn begin(&mut self, id: u32, aux: u32, t0_ns: u64) -> &mut Timeline {
        let k = self.next % RING;
        self.next = self.next.wrapping_add(1);
        let old = self.recs[k];
        if old.slow && !old.printed {
            self.lost = self.lost.saturating_add(1);
        }
        let mut stamps = [0; STAMPS];
        stamps[0] = t0_ns;
        self.recs[k] = Timeline { id, aux, stamps, ..Timeline::default() };
        &mut self.recs[k]
    }

    /// The newest record of frame `(id, aux)`.
    pub fn find_mut(&mut self, id: u32, aux: u32) -> Option<&mut Timeline> {
        (0..RING)
            .map(|back| (self.next + RING - 1 - back) % RING)
            .find(|&k| {
                self.recs[k].id == id && self.recs[k].aux == aux && self.recs[k].stamps[0] != 0
            })
            .map(move |k| &mut self.recs[k])
    }

    /// The slow records not printed yet, oldest first, at most `max`: each is marked printed
    /// as `print` takes it.
    pub fn flush_slow(&mut self, max: u32, mut print: impl FnMut(&Timeline)) -> u32 {
        let mut n = 0;
        for back in (0..RING).rev() {
            if n >= max {
                break;
            }
            let k = (self.next + RING - 1 - back) % RING;
            let r = &mut self.recs[k];
            if r.slow && !r.printed && r.stamps[0] != 0 {
                print(r);
                r.printed = true;
                n += 1;
            }
        }
        n
    }

    /// The slow frames lost since the last call (and a fresh count).
    pub fn take_lost(&mut self) -> u32 {
        core::mem::take(&mut self.lost)
    }
}

/// How a hop's times spread over a window: [`BUCKETS_US`] + one above.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Hist {
    buckets: [u32; 8],
}

impl Hist {
    /// One pass of `us` µs.
    pub fn note(&mut self, us: u64) {
        let k = BUCKETS_US.iter().position(|&b| us < b).unwrap_or(BUCKETS_US.len());
        self.buckets[k] = self.buckets[k].saturating_add(1);
    }

    /// `c0/c1/…/c7`, and a fresh window.
    pub fn take(&mut self) -> [u32; 8] {
        core::mem::take(&mut self.buckets)
    }
}

/// app-host's part of a frame is slow when its input took more than a period to be sent.
pub const fn apphost_slow(in_sent_us: u64, period_us: u64) -> bool {
    in_sent_us > period_us
}

/// windowd's part is slow when its present landed more than two steps and a half period after
/// it was sent, or it took more than a period to be sent.
pub const fn windowd_slow(
    sent_shown_us: u64,
    rx_sent_us: u64,
    step_us: u64,
    period_us: u64,
) -> bool {
    sent_shown_us > 2 * step_us + period_us / 2 || rx_sent_us > period_us
}

/// gpud's part is slow when it missed its slot, landed more than two periods after it was
/// received, or its work passed the cadence threshold.
pub const fn gpud_slow(missed_slot: bool, rx_shown_us: u64, work_us: u64, period_us: u64) -> bool {
    missed_slot || rx_shown_us > 2 * period_us || work_us > CADENCE_DRAW_US
}

/// A hop line in a fixed buffer: fields only from [`VOCABULARY`], cut at [`LINE_CAP`] (`cut`).
#[derive(Clone, Copy, Debug)]
pub struct Line {
    buf: [u8; LINE_CAP],
    len: usize,
    cut: bool,
}

/// A field name outside [`VOCABULARY`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnknownField;

impl Line {
    /// `<head> (` — e.g. `gpud: hops one`.
    pub fn new(head: &str) -> Self {
        let mut line = Self { buf: [0; LINE_CAP], len: 0, cut: false };
        line.push(head);
        line.push(" (");
        line
    }

    fn push(&mut self, s: &str) {
        for &b in s.as_bytes() {
            if self.len >= LINE_CAP - 1 {
                self.cut = true;
                return;
            }
            self.buf[self.len] = b;
            self.len += 1;
        }
    }

    /// ` name=value` (the first field without the space).
    pub fn field(
        &mut self,
        name: &str,
        value: impl core::fmt::Display,
    ) -> Result<(), UnknownField> {
        if !VOCABULARY.contains(&name) {
            return Err(UnknownField);
        }
        if !self.as_str().ends_with('(') {
            self.push(" ");
        }
        self.push(name);
        self.push("=");
        let _ = write!(self, "{value}");
        Ok(())
    }

    /// The line closed with `)` — `None` when it was cut (a cut line is never printed as whole).
    pub fn finish(mut self) -> Option<Self> {
        self.push(")");
        (!self.cut).then_some(self)
    }

    /// The text so far.
    pub fn as_str(&self) -> &str {
        core::str::from_utf8(&self.buf[..self.len]).unwrap_or("")
    }

    /// Whether the line hit the cap.
    pub const fn was_cut(&self) -> bool {
        self.cut
    }
}

impl core::fmt::Write for Line {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        self.push(s);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_slow_rules_hold_their_bounds() {
        let p = 16_667;
        assert!(!apphost_slow(p, p) && apphost_slow(p + 1, p));
        assert!(!windowd_slow(2 * p + p / 2, p, p, p) && windowd_slow(2 * p + p / 2 + 1, 0, p, p));
        assert!(windowd_slow(0, p + 1, p, p), "a present that took more than a period to be sent");
        assert!(gpud_slow(true, 0, 0, p), "a missed slot");
        assert!(!gpud_slow(false, 2 * p, CADENCE_DRAW_US, p) && gpud_slow(false, 2 * p + 1, 0, p));
        assert!(gpud_slow(false, 0, CADENCE_DRAW_US + 1, p), "work past the cadence threshold");
    }

    #[test]
    fn a_timeline_names_its_hops_and_span() {
        let mut ring = Ring::new();
        let r = ring.begin(7, 0, 1_000_000);
        r.stamp(1, 1_400_000);
        r.stamp(2, 3_000_000);
        r.note_wait(250);
        assert_eq!((r.hop_us(0, 1), r.hop_us(1, 2), r.span_us()), (Some(400), Some(1600), 2000));
        assert_eq!(r.hop_us(2, 5), None, "a stamp not taken");
        assert_eq!((r.waits, r.wait_us), (1, 250));
    }

    #[test]
    fn the_ring_prints_a_slow_frame_once_and_counts_what_it_lost() {
        let mut ring = Ring::new();
        ring.begin(1, 0, 10).slow = true;
        let mut seen = 0;
        assert_eq!(ring.flush_slow(8, |_| seen += 1), 1);
        assert_eq!(ring.flush_slow(8, |_| seen += 1), 0, "printed once");
        assert_eq!(seen, 1);
        // A slow frame overwritten before it was printed is lost.
        ring.begin(2, 0, 20).slow = true;
        for k in 0..RING as u32 {
            ring.begin(100 + k, 0, 30 + u64::from(k));
        }
        assert_eq!(ring.take_lost(), 1);
        assert_eq!(ring.take_lost(), 0);
        assert!(ring.find_mut(100 + RING as u32 - 1, 0).is_some());
    }

    #[test]
    fn the_histogram_buckets_by_the_frame_bounds() {
        let mut h = Hist::default();
        for us in [500, 1_500, 16_666, 16_667, 70_000] {
            h.note(us);
        }
        assert_eq!(h.take(), [1, 1, 0, 0, 1, 1, 0, 1]);
    }

    #[test]
    fn test_reject_a_field_outside_the_hop_vocabulary() {
        let mut line = Line::new("gpud: hops one");
        assert!(line.field("f", 7).is_ok());
        assert_eq!(line.field("keystroke", 1), Err(UnknownField), "only times and counts");
        assert_eq!(line.finish().map(|l| l.as_str().len()), Some("gpud: hops one (f=7)".len()));
    }

    #[test]
    fn test_reject_a_line_past_the_console_cap() {
        let mut line = Line::new("windowd: hops one");
        for k in 0..64u64 {
            let _ = line.field("rects", k * 1_000_000);
        }
        assert!(line.was_cut() && line.as_str().len() < LINE_CAP);
        assert!(line.finish().is_none(), "a cut line is never printed as whole");
    }

    /// The worst gpud line the services print stays under the cap.
    #[test]
    fn the_worst_gpud_line_fits_the_cap() {
        let mut line = Line::new("gpud: hops one");
        // The bounds the services print at: t0 in µs since boot (11 digits: ~27 h), hops
        // saturated at `hop_print`, a frame's counts in three digits, pixels and bytes in seven.
        let hops = ["rx>plan", "plan>emit", "emit>sub", "sub>gpu", "gpu>latch", "latch>shown"];
        assert!(line.field("f", u32::MAX).is_ok());
        assert!(line.field("t0", 99_999_999_999u64).is_ok());
        for name in hops {
            assert!(line.field(name, hop_print(u64::MAX)).is_ok());
        }
        assert!(line.field("w", "999/999999").is_ok());
        for name in ["y", "passes", "draws", "gw", "whole"] {
            assert!(line.field(name, 999).is_ok());
        }
        // `slot_px`/`scene_px` stay on the window's `gpu work` line: per frame they pushed the
        // worst line to 283 bytes.
        for name in ["cp", "wpx"] {
            assert!(line.field(name, 9_999_999).is_ok());
        }
        assert!(line.field("by", "999/999/999/999/999/999").is_ok());
        assert_eq!(hop_print(2_000_000), HOP_PRINT_MAX_US, "a stall prints saturated");
        assert!(line.finish().is_some(), "the worst gpud line is cut at {LINE_CAP}");
    }
}
