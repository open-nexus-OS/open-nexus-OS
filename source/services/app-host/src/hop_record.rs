// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: app-host's part of one frame's timeline (RFC-0108 §hops amended, TASK-0373 P1): per
//! present a [`Timeline`] keyed `s=<surface>.<seq>` — the seq windowd receives — with the input
//! it answers received (the stamp alone, never the event), its last dispatch and layout done,
//! its raster done and the present sent; the wait for the previous present's credit, the bytes
//! copied into the surface's slots, the rows and pixels painted. A frame answering an input is
//! slow when it took more than a period to leave (`timeline::apphost_slow`); a slow frame prints
//! once — `apphost: hops one (…)` — when no present left or was acked for [`IDLE_NS`], or at the
//! input window's line (`apphost: hops hist (…)`, 16 inputs), at most `ONE_PER_WINDOW` a window.
//! Pure: `probe/hop_trace.rs` feeds the stamps and prints; nothing here allocates, formats or
//! prints on the frame path. Same one-cfg shape as `band_math` (`probe/` is RISC-V-only).
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: unit tests below (the record's chain order, the credit wait, the idle deadline,
//!   the worst line under the cap)
//! INVARIANTS: times, counts and frame ids only — never what an input was; the ring and the
//!   frame being built are fixed; a record's stamps are in chain order (a stamp older than its
//!   predecessor takes the predecessor's time), so `nx hops` sums them to the frame's span.

use core::fmt::{self, Write as _};

use nexus_ux_budgets::timeline::{
    apphost_slow, hop_print, Hist, Line, Ring, Timeline, ONE_PER_WINDOW, STAMPS,
};

/// No present left or was acked for this long: app-host is idle, its slow frames print.
pub(crate) const IDLE_NS: u64 = 100_000_000;
/// A histogram bucket prints saturated at this count (a window holds 16 inputs).
const BUCKET_PRINT_MAX: u32 = 99;

/// Stamps of a record, in chain order (`IN` is `t0` for a frame answering an input; otherwise
/// `t0` is the frame's first stamp and `in>disp` is not printed).
const IN: usize = 0;
const DISP: usize = 1;
const LAYOUT: usize = 2;
const RASTER: usize = 3;
const SENT: usize = 4;
/// Counts of a record.
const ROWS: usize = 0;
const PX: usize = 1;
const ANSWERS: usize = 2;

/// The hops a record prints and a histogram keeps, by stamp pair.
const HOPS: [(&str, usize, usize); 4] = [
    ("in>disp", IN, DISP),
    ("disp>layout", DISP, LAYOUT),
    ("layout>raster", LAYOUT, RASTER),
    ("raster>sent", RASTER, SENT),
];

/// app-host's ring, the frame being built and the windows' state.
pub(crate) struct AppHops {
    ring: Ring,
    next: Timeline,
    hist: [Hist; HOPS.len()],
    one: u32,
    owed_print: bool,
    cost_owed: bool,
    /// The last present sent or acked, and the idle deadline armed (0: none).
    active_ns: u64,
    idle_at: u64,
}

impl AppHops {
    pub(crate) fn new() -> Self {
        Self {
            ring: Ring::new(),
            next: Timeline::default(),
            hist: [Hist::default(); HOPS.len()],
            one: 0,
            owed_print: false,
            cost_owed: true,
            active_ns: 0,
            idle_at: 0,
        }
    }

    /// A dispatch that changed the model ended at `now_ns` (`layout`: and no relayout ran in it —
    /// its layout is its dispatch).
    pub(crate) fn dispatched(&mut self, now_ns: u64, layout: bool) {
        self.next.stamp(DISP, now_ns);
        if layout {
            self.next.stamp(LAYOUT, now_ns);
        }
    }

    /// A relayout ended at `now_ns`.
    pub(crate) fn laid_out(&mut self, now_ns: u64) {
        self.next.stamp(LAYOUT, now_ns);
    }

    /// `rows` rows — `px` pixels — were painted for the frame being built.
    pub(crate) fn painted(&mut self, rows: u32, px: u64) {
        let c = &mut self.next.counts;
        c[ROWS] = c[ROWS].saturating_add(u64::from(rows));
        c[PX] = c[PX].saturating_add(px);
    }

    /// The frame's raster starts at `now_ns`: what no dispatch and no layout stamped starts here.
    pub(crate) fn frame_begin(&mut self, now_ns: u64) {
        for k in [DISP, LAYOUT] {
            if self.next.stamps[k] == 0 {
                self.next.stamp(k, now_ns);
            }
        }
    }

    /// The frame's raster is done at `now_ns`.
    pub(crate) fn rastered(&mut self, now_ns: u64) {
        self.next.stamp(RASTER, now_ns);
    }

    /// The frame showed nothing: no present, its stamps and counts belonged to nothing.
    pub(crate) fn nothing_shown(&mut self) {
        self.next = Timeline::default();
    }

    /// The present in flight was acked at `now_ns`: the frame being built — work stamped since
    /// the last present, or the input `input_rx` waiting — waited for this credit.
    pub(crate) fn acked(&mut self, now_ns: u64, input_rx: Option<u64>) {
        self.active_ns = now_ns;
        let s = &self.next.stamps;
        let waiting = [input_rx.unwrap_or(0), s[DISP], s[LAYOUT]].into_iter().filter(|&t| t != 0);
        if let Some(since) = waiting.min().filter(|&t| t < now_ns) {
            self.next.note_wait((now_ns - since) / 1000);
        }
    }

    /// The present `seq` of `surface` left at `now_ns`, answering the input received at
    /// `input_rx` (`None`: none). Its record takes what the frame gathered; the next starts.
    pub(crate) fn sent(&mut self, (surface, seq): (u32, u32), now_ns: u64, input_rx: Option<u64>) {
        let next = core::mem::take(&mut self.next);
        let input = input_rx.filter(|&rx| rx != 0 && rx <= now_ns);
        let mut stamps = next.stamps;
        stamps[SENT] = now_ns;
        let first = [DISP, LAYOUT, RASTER].into_iter().map(|k| stamps[k]).find(|&t| t != 0);
        let t0 = input.or(first).unwrap_or(now_ns);
        stamps[IN] = t0;
        let mut at = t0;
        for s in &mut stamps[DISP..=SENT] {
            at = at.max(*s);
            *s = at;
        }
        let r = self.ring.begin(seq, surface, t0);
        r.stamps = stamps;
        (r.waits, r.wait_us, r.copied) = (next.waits, next.wait_us, next.copied);
        (r.counts[ROWS], r.counts[PX]) = (next.counts[ROWS], next.counts[PX]);
        r.counts[ANSWERS] = u64::from(input.is_some());
        let in_sent = r.hop_us(IN, SENT).unwrap_or(0);
        r.slow = input.is_some() && apphost_slow(in_sent, nexus_ux_budgets::FRAME_US);
        if input.is_some() {
            for (h, (_, a, b)) in self.hist.iter_mut().zip(HOPS) {
                if let Some(us) = r.hop_us(a, b) {
                    h.note(us);
                }
            }
        }
        if r.slow {
            self.owed_print = true;
            if self.idle_at == 0 {
                self.idle_at = now_ns + IDLE_NS;
            }
        }
        self.active_ns = now_ns;
    }

    /// `bytes` were copied into the slot the frame is drawn into.
    pub(crate) fn copied(&mut self, bytes: u64) {
        self.next.copied = self.next.copied.saturating_add(bytes);
    }

    /// When the loop's one-shot should wake it to print what is slow (`None`: nothing owed). Set
    /// once by a slow frame, moved at most once per [`IDLE_NS`] while presents go on.
    pub(crate) fn idle_deadline(&self) -> Option<u64> {
        (self.owed_print && self.idle_at != 0).then_some(self.idle_at)
    }

    /// The one-shot fired at `now_ns`: idle for [`IDLE_NS`], the slow frames go to `print`;
    /// else the deadline moves to [`IDLE_NS`] after the last activity.
    pub(crate) fn idle_fired(&mut self, now_ns: u64, print: &mut dyn FnMut(&str)) {
        if self.idle_at == 0 || now_ns < self.idle_at {
            return;
        }
        if now_ns.saturating_sub(self.active_ns) < IDLE_NS {
            self.idle_at = self.active_ns + IDLE_NS;
            return;
        }
        self.flush(print);
    }

    /// The input window's line: the histogram, the slow frames not printed yet, and — once —
    /// what a record and a line cost (`clock`: the kernel clock, ns).
    pub(crate) fn window(&mut self, clock: &dyn Fn() -> u64, print: &mut dyn FnMut(&str)) {
        self.flush(print);
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
        // Nothing owed: the one-shot is not armed for it (the next slow frame arms it anew).
        (self.owed_print, self.idle_at) = (false, 0);
    }
}

/// `c0/c1/…/c7`, each saturated at [`BUCKET_PRINT_MAX`].
struct Buckets([u32; 8]);

impl fmt::Display for Buckets {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (k, c) in self.0.iter().enumerate() {
            if k > 0 {
                f.write_char('/')?;
            }
            write!(f, "{}", (*c).min(BUCKET_PRINT_MAX))?;
        }
        Ok(())
    }
}

/// `apphost: hops one (…)` of record `r`.
fn one_line(r: &Timeline) -> Option<Line> {
    let mut l = Line::new("apphost: hops one");
    l.field("s", format_args!("{}.{}", r.aux, r.id)).ok()?;
    l.field("t0", r.stamps[IN] / 1000).ok()?;
    let skip = usize::from(r.counts[ANSWERS] == 0);
    for (name, a, b) in &HOPS[skip..] {
        if let Some(us) = r.hop_us(*a, *b) {
            l.field(name, hop_print(us)).ok()?;
        }
    }
    l.field("w", format_args!("{}/{}", r.waits.min(999), hop_print(r.wait_us))).ok()?;
    l.field("y", r.yields.min(999)).ok()?;
    l.field("cp", r.copied).ok()?;
    l.field("rows", r.counts[ROWS]).ok()?;
    l.field("px", r.counts[PX]).ok()?;
    l.finish()
}

/// `apphost: hops hist (…)`: the window's hops, the lines printed and the frames lost.
fn hist_line(hist: &mut [Hist; HOPS.len()], one: u32, lost: u32) -> Option<Line> {
    let mut l = Line::new("apphost: hops hist");
    for (h, (name, _, _)) in hist.iter_mut().zip(HOPS) {
        l.field(name, Buckets(h.take())).ok()?;
    }
    l.field("one", one).ok()?;
    l.field("lost", lost.min(9_999)).ok()?;
    l.finish()
}

/// `apphost: hops cost (…)`: a clock read, a record (its stamps and its write) and the window
/// line just printed — measured, once.
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
    let mut l = Line::new("apphost: hops cost");
    write!(l, "nsec_ns={nsec_ns} record_ns={record_ns} line_us={line_us}").ok()?;
    l.finish()
}

#[cfg(test)]
#[path = "hop_record_tests.rs"]
mod tests;
