// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: windowd's part of one frame's timeline (RFC-0108 §hops amended, TASK-0373 P1): per
//! present to gpud a [`Timeline`] keyed by its seq `f` — the client presents it folded in
//! (`from=<surface>.<seq>`, at most [`FROM_NAMED`] by id and `+n`), when the newest of them was
//! received, when the frame became owed, when its scene was built, sent, acked and shown (the
//! ack's `Shown.at_ns`), the loop's wait for a ring slot between owed and sent, the base bytes
//! rewritten for it, its layers and rects. A frame is slow by `timeline::windowd_slow`; a slow
//! frame prints once — `windowd: hops one (…)` — when windowd had no frame in flight for
//! [`IDLE_NS`] or at its window line (`windowd: hops hist (…)`, a second with traffic), at most
//! `ONE_PER_WINDOW` a window. Pure: the runtime (`compositor/runtime/frame_pulse.rs`) feeds the
//! stamps and prints; nothing here allocates, formats or prints on the frame path.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: unit tests below (the seq map: a present folded into two frames, more clients
//!   than named ids; the worst line under the cap; slow, idle and window printing)
//! INVARIANTS: times, counts and frame ids only — never what an input was; the ring, the fold
//!   and the presents in flight are fixed arrays; a record's stamps are in chain order (a stamp
//!   older than its predecessor takes the predecessor's time), so `nx hops` sums them to the
//!   frame's span.

use core::fmt::{self, Write as _};

use nexus_ux_budgets::timeline::{
    hop_print, windowd_slow, Hist, Line, Ring, Timeline, LINE_CAP, ONE_PER_WINDOW,
};

/// The client presents a frame names by id; the rest are counted (`+n`).
pub(crate) const FROM_NAMED: usize = 4;
/// Presents to gpud kept in flight, as `FrameClock` keeps its predictions.
const FLIGHT: usize = 4;
/// No frame in flight for this long: windowd is idle, its slow frames print.
pub(crate) const IDLE_NS: u64 = 100_000_000;
/// A histogram bucket prints saturated at this count (a window holds far fewer frames).
const BUCKET_PRINT_MAX: u32 = 9_999;

/// Stamps of a record, in chain order.
const RX: usize = 0;
const OWED: usize = 1;
const SCENE: usize = 2;
const SENT: usize = 3;
const ACKED: usize = 4;
/// The ack's `Shown.at_ns` (absolute, not a hop).
const SHOWN: usize = 5;
/// Counts of a record: layers, rects, the named ids and the overflow, the ids packed.
const LAYERS: usize = 0;
const RECTS: usize = 1;
const FOLDED: usize = 2;
const IDS: usize = 3;
const _: () = assert!(IDS + FROM_NAMED <= nexus_ux_budgets::timeline::COUNTS);

/// The hops a histogram keeps, by stamp pair.
const HOPS: [(&str, usize, usize); 4] = [
    ("rx>owed", RX, OWED),
    ("owed>scene", OWED, SCENE),
    ("scene>sent", SCENE, SENT),
    ("sent>acked", SENT, ACKED),
];

/// The client presents folded into one present to gpud.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Fold {
    ids: [(u32, u32); FROM_NAMED],
    named: u8,
    more: u32,
    /// When the newest of them was received (0: none) — the frame's `t0`: every app-host record
    /// it names was sent before it.
    last_rx_ns: u64,
}

impl Fold {
    fn add(&mut self, id: (u32, u32), rx_ns: u64) {
        if self.ids[..usize::from(self.named)].contains(&id) {
            return;
        }
        match self.ids.get_mut(usize::from(self.named)) {
            Some(slot) => {
                *slot = id;
                self.named += 1;
            }
            None => self.more = self.more.saturating_add(1),
        }
        self.last_rx_ns = self.last_rx_ns.max(rx_ns);
    }

    /// `other`'s presents folded in too (a frame gpud refused is drawn again by the next).
    fn merge(&mut self, other: &Fold) {
        for &id in &other.ids[..usize::from(other.named)] {
            self.add(id, 0);
        }
        self.more = self.more.saturating_add(other.more);
        if self.last_rx_ns == 0 {
            self.last_rx_ns = other.last_rx_ns;
        }
    }

    fn pack(&self, counts: &mut [u64]) {
        counts[FOLDED] = (u64::from(self.named) << 32) | u64::from(self.more);
        for (c, (s, q)) in counts[IDS..IDS + FROM_NAMED].iter_mut().zip(self.ids) {
            *c = (u64::from(s) << 32) | u64::from(q);
        }
    }
}

/// windowd's ring, the fold of the frame being built and the presents in flight.
pub(crate) struct HopTrace {
    ring: Ring,
    fold: Fold,
    flight: [(u32, Fold); FLIGHT],
    scene_ns: u64,
    layers: u64,
    rects: u64,
    /// The loop's wait for a ring slot while the next frame is owed: acks, µs, and since when.
    waits: (u32, u64),
    wait_mark: u64,
    hist: [Hist; HOPS.len()],
    /// `hops one` lines printed in this window; slow records not printed yet.
    one: u32,
    owed_print: bool,
    landed_ns: u64,
    cost_owed: bool,
}

impl HopTrace {
    pub(crate) fn new() -> Self {
        Self {
            ring: Ring::new(),
            fold: Fold::default(),
            flight: [(0, Fold::default()); FLIGHT],
            scene_ns: 0,
            layers: 0,
            rects: 0,
            waits: (0, 0),
            wait_mark: 0,
            hist: [Hist::default(); HOPS.len()],
            one: 0,
            owed_print: false,
            landed_ns: 0,
            cost_owed: true,
        }
    }

    /// A client present of `surface` with `seq` was accepted at `rx_ns`.
    pub(crate) fn client_present(&mut self, surface: u32, seq: u32, rx_ns: u64) {
        self.fold.add((surface, seq), rx_ns);
    }

    /// The scene of the frame being sent was built at `now_ns`: `layers` drawn, `rects` named.
    pub(crate) fn scene(&mut self, now_ns: u64, layers: usize, rects: usize) {
        self.scene_ns = now_ns;
        (self.layers, self.rects) = (layers as u64, rects as u64);
    }

    /// The present `f` left for gpud at `now_ns`; it was owed since `owed` and `copied` base
    /// bytes were rewritten for it. Its record starts; the fold goes with it.
    pub(crate) fn sent(&mut self, f: u32, owed: Option<u64>, now_ns: u64, copied: u64) {
        let fold = core::mem::take(&mut self.fold);
        let first = [fold.last_rx_ns, owed.unwrap_or(0), self.scene_ns, now_ns];
        let t0 = first.into_iter().find(|&t| t != 0).unwrap_or(now_ns);
        let r = self.ring.begin(f, 0, t0);
        // Chain order: a frame owed before its newest present arrived is owed from then.
        let mut at = t0;
        for (k, t) in [(OWED, owed.unwrap_or(0)), (SCENE, self.scene_ns), (SENT, now_ns)] {
            at = at.max(t);
            r.stamp(k, at);
        }
        (r.waits, r.wait_us) = self.waits;
        r.copied = copied;
        (r.counts[LAYERS], r.counts[RECTS]) = (self.layers, self.rects);
        fold.pack(&mut r.counts);
        self.flight[f as usize % FLIGHT] = (f, fold);
        (self.scene_ns, self.layers, self.rects, self.waits, self.wait_mark) = (0, 0, 0, (0, 0), 0);
    }

    /// gpud credited the present `f` at `now_ns` — refused (`ok == false`: drawn again by the
    /// next frame, which then names its presents too) or shown at `shown` (`at_ns`, the step
    /// and the period in µs; `None`: an untimed ack). `owed`: the next frame is owed since then
    /// and could not leave — the loop waited for this ack.
    pub(crate) fn acked(
        &mut self,
        f: u32,
        now_ns: u64,
        (ok, shown): (bool, Option<(u64, u64, u64)>),
        owed: Option<u64>,
    ) {
        if let Some(since) = owed {
            let from = since.max(self.wait_mark);
            self.waits.0 = self.waits.0.saturating_add(1);
            self.waits.1 = self.waits.1.saturating_add(now_ns.saturating_sub(from) / 1000);
            self.wait_mark = now_ns;
        }
        self.landed_ns = now_ns;
        let (kept, fold) = self.flight[f as usize % FLIGHT];
        if !ok && kept == f {
            self.fold.merge(&fold);
        }
        let Some(r) = self.ring.find_mut(f, 0) else { return };
        r.stamp(ACKED, now_ns.max(r.stamps[SENT]));
        let p = nexus_ux_budgets::FRAME_US;
        let (landed, step_us, period_us) = shown.unwrap_or((now_ns, p, p));
        r.stamp(SHOWN, landed);
        let sent_shown = landed.saturating_sub(r.stamps[SENT]) / 1000;
        let rx_sent = r.hop_us(RX, SENT).unwrap_or(0);
        r.slow = windowd_slow(sent_shown, rx_sent, step_us, period_us);
        for (h, (_, a, b)) in self.hist.iter_mut().zip(HOPS) {
            if let Some(us) = r.hop_us(a, b) {
                h.note(us);
            }
        }
        self.owed_print |= r.slow;
    }

    /// The loop's pass at `now_ns` with `in_flight` presents out: when windowd was idle for
    /// [`IDLE_NS`], the slow frames not printed yet go to `print` (within the window's budget).
    pub(crate) fn idle(&mut self, now_ns: u64, in_flight: u32, print: &mut dyn FnMut(&str)) {
        let idle = in_flight == 0 && now_ns.saturating_sub(self.landed_ns) >= IDLE_NS;
        if idle && self.owed_print {
            self.flush(print);
        }
    }

    /// The window line (a second with traffic): the histogram, the slow frames not printed
    /// yet, and — once — what a record and a line cost (`clock`: the kernel clock, ns).
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
        self.owed_print = false;
    }
}

/// `c0/c1/…/c7`, each saturated at `max`.
struct Buckets([u32; 8], u32);

impl fmt::Display for Buckets {
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

/// The `from=` list: ids while `room` bytes hold them, the rest `+n`.
struct From {
    counts: [u64; FROM_NAMED + 1],
    room: usize,
}

impl fmt::Display for From {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (named, mut more) = ((self.counts[0] >> 32) as usize, self.counts[0] as u32);
        // `+` and ten digits stay free for the overflow.
        let (mut used, budget) = (0usize, self.room.saturating_sub(11));
        for (k, id) in self.counts[1..=named.min(FROM_NAMED)].iter().enumerate() {
            let (s, q) = ((id >> 32) as u32, *id as u32);
            let len = digits(s) + 1 + digits(q) + usize::from(k > 0);
            if used + len > budget {
                more = more.saturating_add(1);
                continue;
            }
            if k > 0 {
                f.write_char(',')?;
            }
            write!(f, "{s}.{q}")?;
            used += len;
        }
        if more > 0 {
            write!(f, "+{more}")?;
        }
        Ok(())
    }
}

fn digits(v: u32) -> usize {
    v.checked_ilog10().map_or(1, |d| d as usize + 1)
}

/// `windowd: hops one (…)` of record `r` — `from=` last, in the room the line has left.
pub(crate) fn one_line(r: &Timeline) -> Option<Line> {
    let mut l = Line::new("windowd: hops one");
    l.field("f", r.id).ok()?;
    l.field("t0", r.stamps[RX] / 1000).ok()?;
    for (name, a, b) in HOPS {
        if let Some(us) = r.hop_us(a, b) {
            l.field(name, hop_print(us)).ok()?;
        }
    }
    if r.stamps[SHOWN] != 0 {
        l.field("shown", r.stamps[SHOWN] / 1000).ok()?;
    }
    l.field("w", format_args!("{}/{}", r.waits.min(999), hop_print(r.wait_us))).ok()?;
    l.field("y", r.yields.min(999)).ok()?;
    l.field("cp", r.copied).ok()?;
    l.field("layers", r.counts[LAYERS]).ok()?;
    l.field("rects", r.counts[RECTS]).ok()?;
    if r.counts[FOLDED] != 0 {
        let mut counts = [0; FROM_NAMED + 1];
        counts[0] = r.counts[FOLDED];
        counts[1..].copy_from_slice(&r.counts[IDS..IDS + FROM_NAMED]);
        let room = (LINE_CAP - 2).saturating_sub(l.as_str().len() + " from=".len());
        l.field("from", From { counts, room }).ok()?;
    }
    l.finish()
}

/// `windowd: hops hist (…)`: the window's hops, the lines printed and the frames lost.
fn hist_line(hist: &mut [Hist; HOPS.len()], one: u32, lost: u32) -> Option<Line> {
    let mut l = Line::new("windowd: hops hist");
    for (h, (name, _, _)) in hist.iter_mut().zip(HOPS) {
        l.field(name, Buckets(h.take(), BUCKET_PRINT_MAX)).ok()?;
    }
    l.field("one", one).ok()?;
    l.field("lost", lost.min(BUCKET_PRINT_MAX)).ok()?;
    l.finish()
}

/// `windowd: hops cost (…)`: a clock read, a record (its stamps and its write) and the window
/// line just printed — measured, once.
pub(crate) fn cost_line(clock: &dyn Fn() -> u64, line_us: u64) -> Option<Line> {
    const READS: u64 = 64;
    let t = clock();
    for _ in 0..READS {
        core::hint::black_box(clock());
    }
    let nsec_ns = clock().saturating_sub(t) / (READS + 1);
    let t = clock();
    let mut r = Timeline::default();
    for k in 0..nexus_ux_budgets::timeline::STAMPS {
        r.stamp(k, clock());
    }
    core::hint::black_box(&mut r);
    let record_ns = clock().saturating_sub(t);
    let mut l = Line::new("windowd: hops cost");
    write!(l, "nsec_ns={nsec_ns} record_ns={record_ns} line_us={line_us}").ok()?;
    l.finish()
}

#[cfg(test)]
#[path = "hop_trace_tests.rs"]
mod tests;
