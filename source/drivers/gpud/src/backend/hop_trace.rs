// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: gpud's part of one frame's timeline (RFC-0108 §hops amended, TASK-0373 P1) on the
//! board's display: per present a [`Timeline`] keyed by the seq its ack echoes — received, the
//! frame's plan, its passes' emits and submits laid end to end, the GPU done (the frame's
//! `split`, `gpu/desktop.rs`, handed over as a [`FrameHop`]), latched and shown (`dc/flip.rs`);
//! the pass waits, the bytes written into the pass memory, the passes, draws, glass writes, the
//! writes of a whole glass and their pixels, the writes by origin class. The record starts when
//! the present is drawn (`present_ack_deferred`) and is completed at its landing; a frame is slow
//! by `timeline::gpud_slow` and prints once — `gpud: hops one (…)` — when gpud received and
//! landed nothing for [`IDLE_NS`], or at its window line (`gpud: hops hist (…)`, 32 landings).
//! Pure: the display controller feeds the stamps and prints; nothing here allocates, formats or
//! prints on the frame path.
//! OWNERS: @gpu @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: unit tests in `hop_trace_tests.rs` (the chain order, the CPU frame, the
//!   landing, the window, the worst lines under the cap)
//! INVARIANTS: times, counts and frame ids only; the ring and the frame handed over are fixed;
//!   a record's stamps are in chain order (a stamp older than its predecessor takes the
//!   predecessor's time), so `nx hops` sums them to the frame's span.

use core::fmt::{self, Write as _};

use nexus_ux_budgets::timeline::{
    gpud_slow, hop_print, Hist, Line, Ring, Timeline, ONE_PER_WINDOW, STAMPS,
};

/// Nothing received and nothing landed for this long: gpud is idle, its slow frames print.
pub(crate) const IDLE_NS: u64 = 100_000_000;
/// Landings a histogram window holds (as the `gpud: hops frame` window).
const WINDOW: u32 = nexus_ux_budgets::FRAME_WINDOW;
/// A histogram bucket prints saturated here (a window holds 32 landings).
const BUCKET_PRINT_MAX: u32 = 99;
/// A frame's counts, its draws and its bytes print saturated here (the worst line stays under
/// the cap: `the_worst_gpud_lines_fit_the_cap_and_name_only_hop_fields`).
const COUNT_PRINT_MAX: u64 = 999;
const DRAWS_PRINT_MAX: u64 = 9_999;
const BYTES_PRINT_MAX: u64 = 99_999_999;

/// Stamps of a record, in chain order.
const RX: usize = 0;
const PLAN: usize = 1;
const EMIT: usize = 2;
const SUB: usize = 3;
const GPU: usize = 4;
const LATCH: usize = 5;
const SHOWN: usize = 6;
/// Counts of a record (`BY`: the six classes, ten bits each).
const PASSES: usize = 0;
const DRAWS: usize = 1;
const GW: usize = 2;
const WHOLE: usize = 3;
const WPX: usize = 4;
const BY: usize = 5;
const BY_BITS: u32 = 10;

/// The hops a record prints and a histogram keeps, by stamp pair.
const HOPS: [(&str, usize, usize); 6] = [
    ("rx>plan", RX, PLAN),
    ("plan>emit", PLAN, EMIT),
    ("emit>sub", EMIT, SUB),
    ("sub>gpu", SUB, GPU),
    ("gpu>latch", GPU, LATCH),
    ("latch>shown", LATCH, SHOWN),
];

/// What the GPU's executor knows of the frame it drew (`gpu/desktop.rs`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct FrameHop {
    /// The plan done; its passes' emits and submits laid end to end after it; the GPU done (ns).
    pub(crate) at: [u64; 4],
    /// The waits for a pass (n, µs).
    pub(crate) waits: (u32, u64),
    /// Bytes written into the pass memory (streams, descriptors).
    pub(crate) copied: u64,
    pub(crate) passes: u32,
    pub(crate) draws: u32,
    /// Glass writes; of them the writes of a whole glass (band `Full`) and their pixels.
    pub(crate) gw: u32,
    pub(crate) whole: u32,
    pub(crate) wpx: u64,
    /// The writes by origin class: below, glass, unknown, moved, order, rest.
    pub(crate) by: [u16; 6],
}

impl FrameHop {
    /// The stamps of a frame whose plan was done at `planned_ns` and whose GPU work finished at
    /// `done_ns`, its `split` (plan, emit, submit, wait, retire — ns) laid end to end.
    pub(crate) fn laid(planned_ns: u64, split: [u64; 5], done_ns: u64) -> [u64; 4] {
        let emitted = planned_ns.saturating_add(split[1]);
        let submitted = emitted.saturating_add(split[2]);
        [planned_ns, emitted, submitted, done_ns.max(submitted)]
    }
}

/// gpud's ring, the frame handed over and the window's state.
pub(crate) struct GpudHops {
    ring: Ring,
    drawn: Option<FrameHop>,
    hist: [Hist; HOPS.len()],
    frames: u32,
    one: u32,
    owed_print: bool,
    cost_owed: bool,
    active_ns: u64,
}

impl GpudHops {
    pub(crate) fn new() -> Self {
        Self {
            ring: Ring::new(),
            drawn: None,
            hist: [Hist::default(); HOPS.len()],
            frames: 0,
            one: 0,
            owed_print: false,
            cost_owed: true,
            active_ns: 0,
        }
    }

    /// The GPU drew the running present (`None`: the CPU did, or nothing).
    pub(crate) fn drawn(&mut self, hop: Option<FrameHop>) {
        self.drawn = hop;
    }

    /// The present `seq`, received at `received_ns`, is drawn at `drawn_ns`: its record starts
    /// with what its frame handed over (a CPU frame's work is all `rx>plan`).
    pub(crate) fn received(&mut self, seq: u32, received_ns: u64, drawn_ns: u64) {
        let hop = self.drawn.take().unwrap_or_default();
        let t0 = if received_ns != 0 { received_ns } else { drawn_ns };
        let r = self.ring.begin(seq, 0, t0);
        let done = if hop.at[3] != 0 { hop.at[3] } else { drawn_ns };
        let mut at = t0;
        for (k, t) in [(PLAN, hop.at[0]), (EMIT, hop.at[1]), (SUB, hop.at[2]), (GPU, done)] {
            at = at.max(t);
            r.stamp(k, at);
        }
        (r.waits, r.wait_us, r.copied) = (hop.waits.0, hop.waits.1, hop.copied);
        let c = &mut r.counts;
        (c[PASSES], c[DRAWS], c[GW]) = (hop.passes.into(), hop.draws.into(), hop.gw.into());
        (c[WHOLE], c[WPX]) = (hop.whole.into(), hop.wpx);
        c[BY] = hop.by.iter().enumerate().fold(0, |acc, (k, n)| {
            acc | (u64::from((*n).min((1 << BY_BITS) - 1)) << (BY_BITS * k as u32))
        });
        self.active_ns = drawn_ns.max(received_ns);
    }

    /// The present `seq` landed at `now_ns`: latched at `latched_ns` and shown at `shown_ns`
    /// (0: acked when drawn — no flip of its own); `missed`: past its slot. Returns whether the
    /// window is full (the caller prints it: [`Self::window`]).
    pub(crate) fn landed(
        &mut self,
        seq: u32,
        (latched_ns, shown_ns): (u64, u64),
        (missed, period_ns): (bool, u64),
        now_ns: u64,
    ) -> bool {
        self.active_ns = now_ns;
        let Some(r) = self.ring.find_mut(seq, 0) else { return false };
        let mut at = r.stamps[GPU];
        for (k, t) in [(LATCH, latched_ns), (SHOWN, shown_ns)] {
            if t != 0 {
                at = at.max(t);
                r.stamp(k, at);
            }
        }
        let work = r.hop_us(RX, GPU).unwrap_or(0);
        let rx_shown = r.hop_us(RX, SHOWN).unwrap_or(work);
        r.slow = gpud_slow(missed, rx_shown, work, period_ns / 1000);
        for (h, (_, a, b)) in self.hist.iter_mut().zip(HOPS) {
            if let Some(us) = r.hop_us(a, b) {
                h.note(us);
            }
        }
        self.owed_print |= r.slow;
        self.frames += 1;
        self.frames >= WINDOW
    }

    /// The loop's pass at `now_ns`: idle for [`IDLE_NS`], the slow frames not printed yet go to
    /// `print` (within the window's budget).
    pub(crate) fn idle(&mut self, now_ns: u64, print: &mut dyn FnMut(&str)) {
        if self.owed_print && now_ns.saturating_sub(self.active_ns) >= IDLE_NS {
            self.flush(print);
        }
    }

    /// The window line (32 landings): the histogram, the slow frames not printed yet, and —
    /// once — what a record and a line cost (`clock`: the kernel clock, ns).
    pub(crate) fn window(&mut self, clock: &dyn Fn() -> u64, print: &mut dyn FnMut(&str)) {
        self.flush(print);
        self.frames = 0;
        let (one, lost) = (core::mem::take(&mut self.one), self.ring.take_lost());
        let t = clock();
        if let Some(line) = hist_line(&mut self.hist, one, lost) {
            print(line.as_str());
        }
        let line_us = clock().saturating_sub(t) / 1000;
        if core::mem::take(&mut self.cost_owed) {
            if let Some(line) = cost_line(clock, line_us) {
                print(line.as_str());
            }
        }
    }

    fn flush(&mut self, print: &mut dyn FnMut(&str)) {
        let room = ONE_PER_WINDOW.saturating_sub(self.one);
        self.one += self.ring.flush_slow(room, |r| {
            if let Some(line) = one_line(r) {
                print(line.as_str());
            }
        });
        self.owed_print = false;
    }
}

/// `n0/n1/…`, each saturated at `max`.
struct Counts<const N: usize>([u64; N], u64);

impl<const N: usize> fmt::Display for Counts<N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (k, c) in self.0.iter().enumerate() {
            if k > 0 {
                f.write_char('/')?;
            }
            write!(f, "{}", (*c).min(self.1))?;
        }
        Ok(())
    }
}

/// `gpud: hops one (…)` of record `r`.
fn one_line(r: &Timeline) -> Option<Line> {
    let mut l = Line::new("gpud: hops one");
    l.field("f", r.id).ok()?;
    l.field("t0", r.stamps[RX] / 1000).ok()?;
    for (name, a, b) in HOPS {
        if let Some(us) = r.hop_us(a, b) {
            l.field(name, hop_print(us)).ok()?;
        }
    }
    l.field("w", format_args!("{}/{}", r.waits.min(999), hop_print(r.wait_us))).ok()?;
    l.field("y", r.yields.min(999)).ok()?;
    l.field("cp", r.copied.min(BYTES_PRINT_MAX)).ok()?;
    let counts = [("passes", PASSES), ("draws", DRAWS), ("gw", GW), ("whole", WHOLE)];
    for (name, k) in counts {
        let max = if k == DRAWS { DRAWS_PRINT_MAX } else { COUNT_PRINT_MAX };
        l.field(name, r.counts[k].min(max)).ok()?;
    }
    l.field("wpx", r.counts[WPX].min(BYTES_PRINT_MAX)).ok()?;
    let by = core::array::from_fn(|k| (r.counts[BY] >> (BY_BITS * k as u32)) & 0x3ff);
    l.field("by", Counts::<6>(by, 999)).ok()?;
    l.finish()
}

/// `gpud: hops hist (…)`: the window's hops, the lines printed and the frames lost.
fn hist_line(hist: &mut [Hist; HOPS.len()], one: u32, lost: u32) -> Option<Line> {
    let mut l = Line::new("gpud: hops hist");
    for (h, (name, _, _)) in hist.iter_mut().zip(HOPS) {
        let b = h.take().map(u64::from);
        l.field(name, Counts::<8>(b, u64::from(BUCKET_PRINT_MAX))).ok()?;
    }
    l.field("one", one).ok()?;
    l.field("lost", lost.min(9_999)).ok()?;
    l.finish()
}

/// `gpud: hops cost (…)`: a clock read, a record (its stamps and its write) and the window line
/// just printed — measured, once.
fn cost_line(clock: &dyn Fn() -> u64, line_us: u64) -> Option<Line> {
    const READS: u64 = 64;
    let t = clock();
    for _ in 0..READS {
        core::hint::black_box(clock());
    }
    let nsec_ns = clock().saturating_sub(t) / (READS + 1);
    let t = clock();
    let mut r = Timeline::default();
    for k in 0..STAMPS {
        r.stamp(k, clock());
    }
    core::hint::black_box(&mut r);
    let record_ns = clock().saturating_sub(t);
    let mut l = Line::new("gpud: hops cost");
    write!(l, "nsec_ns={nsec_ns} record_ns={record_ns} line_us={line_us}").ok()?;
    l.finish()
}

#[cfg(test)]
#[path = "hop_trace_tests.rs"]
mod tests;
