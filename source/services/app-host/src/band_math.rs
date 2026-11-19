// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the pure decisions of a scroll layer's virtual band (RFC-0109): what it draws next —
//! the visible window first, below then above, then the margins ahead and behind, starting over
//! at the window when it holds nothing of it — how its valid range grows as the ring overwrites
//! what it held — and the bounded set of rectangles damage is kept in (TASK-0354 P8): what a
//! present names, and what a model change leaves to repaint (`Pending`). Same one-cfg shape as
//! `hover_wash`: the drawing lives in RISC-V-only `probe/band.rs`, the decisions here where host
//! tests reach them.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `band_math/tests.rs` (host)
//! INVARIANTS: a set of rectangles never covers less than what was added to it (a merge is a
//!   union); a pending `whole` is never narrowed

/// A scroll layer's virtual band: where its ring lives in the store and what it holds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Band {
    pub(crate) bx: u16,
    pub(crate) by: u16,
    pub(crate) strip_len: u16,
    pub(crate) strips: u8,
    /// The content range the ring holds: `[valid_start, valid_start + valid_len)`.
    pub(crate) valid_start: u32,
    pub(crate) valid_len: u32,
}

impl Band {
    pub(crate) fn capacity(&self) -> u32 {
        u32::from(self.strip_len) * u32::from(self.strips)
    }

    pub(crate) fn valid_end(&self) -> u32 {
        self.valid_start + self.valid_len
    }
}

/// What a band draws next: content positions `[from, to)`, adjacent to what it holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Fill {
    pub(crate) from: u32,
    pub(crate) to: u32,
    /// Part of the visible window (drawn first, and in full on the first frame).
    pub(crate) visible: bool,
    /// Grows the valid range at its end (else at its start).
    pub(crate) forward: bool,
    /// Nothing of the visible window is held: the band starts over at the window.
    pub(crate) restart: bool,
}

/// What a band draws next for a viewport `len` long over content `extent` long at `offset` — the
/// visible window first, below then above, then the margins ahead and behind (half the ring's
/// spare positions each way) — or `None` when it holds its target or shows nothing (`covered`).
pub(crate) fn next_fill(
    band: &Band,
    len: u32,
    extent: i32,
    offset: i32,
    covered: bool,
) -> Option<Fill> {
    let cap = band.capacity();
    if covered || cap == 0 || extent <= 0 {
        return None;
    }
    let extent = extent as u32;
    let margin = cap.saturating_sub(len) / 2;
    let off = (offset.max(0) as u32).min(extent);
    let (v0, v1) = (off, (off + len).min(extent));
    let (t0, t1) = (off.saturating_sub(margin), (off + len + margin).min(extent));
    let fill = |from, to, visible, forward| Fill { from, to, visible, forward, restart: false };
    if v1 > v0 && (band.valid_len == 0 || band.valid_end() <= v0 || band.valid_start >= v1) {
        return Some(Fill { restart: true, ..fill(v0, v1, true, true) });
    }
    if band.valid_end() < v1 {
        return Some(fill(band.valid_end(), v1, true, true));
    }
    if band.valid_start > v0 {
        return Some(fill(v0, band.valid_start, true, false));
    }
    if band.valid_end() < t1 {
        return Some(fill(band.valid_end(), t1, false, true));
    }
    if band.valid_start > t0 {
        return Some(fill(t0, band.valid_start, false, false));
    }
    None
}

/// One run of a fill at most `chunk` positions long that crosses no strip end (nor the ring's
/// wrap): its first positions when it grows forward, its last when it grows backward.
pub(crate) fn run_of(fill: &Fill, strip_len: u16, chunk: u32) -> (u32, u32) {
    let strip = u32::from(strip_len).max(1);
    if fill.forward {
        let run = strip - fill.from % strip;
        (fill.from, fill.to.min(fill.from + chunk).min(fill.from + run))
    } else {
        let back = (fill.to - 1) % strip + 1;
        (fill.from.max(fill.to.saturating_sub(chunk)).max(fill.to - back), fill.to)
    }
}

/// The band holds `[q0, q1)` too: grown at its end (what the ring overwrote at its start is gone)
/// or at its start (what it overwrote at its end is gone). A run adjacent to neither is a fresh
/// start — the caller resets the band before drawing it.
pub(crate) fn grow(band: &mut Band, q0: u32, q1: u32) {
    let cap = band.capacity();
    if band.valid_len == 0 {
        band.valid_start = q0;
        band.valid_len = (q1 - q0).min(cap);
    } else if q0 == band.valid_end() {
        let start = band.valid_start.max(q1.saturating_sub(cap));
        band.valid_start = start;
        band.valid_len = q1 - start;
    } else if q1 == band.valid_start {
        let end = band.valid_end().min(q0 + cap);
        band.valid_start = q0;
        band.valid_len = end - q0;
    }
}

/// A rectangle `(x, y, width, height)`.
pub(crate) use crate::slot_math::Rect;

/// The rectangles a set of damage keeps — the wire's bound (`probe/band.rs` asserts the two equal).
pub(crate) const DAMAGE_RECTS: usize = 4;

fn union(a: Rect, b: Rect) -> Rect {
    let (x0, y0) = (a.0.min(b.0), a.1.min(b.1));
    let (x1, y1) = (
        a.0.saturating_add(a.2).max(b.0.saturating_add(b.2)),
        a.1.saturating_add(a.3).max(b.1.saturating_add(b.3)),
    );
    (x0, y0, x1 - x0, y1 - y0)
}

fn area(r: Rect) -> i128 {
    i128::from(r.2) * i128::from(r.3)
}

/// A bounded set of damage rectangles: what a present names (surface and store pixels) and what
/// a model change leaves to repaint ([`Pending`]). ONE merge rule (TASK-0354 P8): a pair whose
/// union is no larger than the two together is one rectangle; above [`DAMAGE_RECTS`], the pair —
/// among all, the newcomer or not — whose union adds the least area. A merge is always a union,
/// so the set never covers less than what was added: over the bound the damage grows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct PresentDamage {
    rects: [Rect; DAMAGE_RECTS],
    n: usize,
}

impl PresentDamage {
    pub(crate) fn add(&mut self, r: Rect) {
        if r.2 == 0 || r.3 == 0 {
            return;
        }
        let mut all = [(0, 0, 0, 0); DAMAGE_RECTS + 1];
        all[..self.n].copy_from_slice(&self.rects[..self.n]);
        all[self.n] = r;
        let mut n = self.n + 1;
        loop {
            // The pair whose union adds the least area over the two.
            let mut best: Option<(i128, usize, usize)> = None;
            for i in 0..n {
                for j in i + 1..n {
                    let grows = area(union(all[i], all[j])) - area(all[i]) - area(all[j]);
                    if best.is_none_or(|b| grows < b.0) {
                        best = Some((grows, i, j));
                    }
                }
            }
            match best {
                Some((grows, i, j)) if grows <= 0 || n > DAMAGE_RECTS => {
                    all[i] = union(all[i], all[j]);
                    all.copy_within(j + 1..n, j);
                    n -= 1;
                }
                _ => break,
            }
        }
        self.rects[..n].copy_from_slice(&all[..n]);
        self.n = n;
    }

    pub(crate) fn rects(&self) -> &[Rect] {
        &self.rects[..self.n]
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.n == 0
    }

    /// The pixels the rectangles name (one two of them share counts twice).
    pub(crate) fn px(&self) -> u64 {
        self.rects().iter().map(|r| u64::from(r.2) * u64::from(r.3)).sum()
    }

    /// The disjoint row intervals `[y0, y1)` the rectangles cover, ascending — what a pass that
    /// works by rows (a band's ring, a motion layer's store rows) runs over once each.
    pub(crate) fn row_intervals(&self) -> ([(i32, i32); DAMAGE_RECTS], usize) {
        let edge = |v: u32| i32::try_from(v).unwrap_or(i32::MAX);
        let mut rows = [(0, 0); DAMAGE_RECTS];
        for (o, r) in rows.iter_mut().zip(self.rects()) {
            *o = (edge(r.1), edge(r.1.saturating_add(r.3)));
        }
        rows[..self.n].sort_unstable();
        let mut m = 0;
        for k in 0..self.n {
            if m > 0 && rows[k].0 <= rows[m - 1].1 {
                rows[m - 1].1 = rows[m - 1].1.max(rows[k].1);
            } else {
                rows[m] = rows[k];
                m += 1;
            }
        }
        (rows, m)
    }
}

/// What the next present repaints of the base (TASK-0354 P8): everything, or a bounded set of
/// rectangles in layout coordinates. The ONE pending damage of a surface — every contributor
/// adds what it changed (the layout's diff, an animated node, a motion layer that went, a widget
/// loop), the present loop takes it. Everything (`whole`) is for doubt alone — a mount, a
/// resize, a theme, a change that names no bound — and is never narrowed by a later rectangle.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Pending {
    whole: bool,
    rects: PresentDamage,
}

/// What a present repaints ([`Pending::take`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Change {
    /// Everything, anew: the base, the patch, every band and motion layer.
    Whole,
    /// These rectangles of the layout.
    Rects(PresentDamage),
}

impl Pending {
    /// Everything changed, or nothing bounds what did.
    pub(crate) fn whole(&mut self) {
        *self = Self { whole: true, rects: PresentDamage::default() };
    }

    /// The rectangle with edges `(x0, y0, x1, y1)` changed — cut to the base `(w, h)`; one that
    /// leaves nothing there names nothing.
    pub(crate) fn add(&mut self, (x0, y0, x1, y1): (i32, i32, i32, i32), (w, h): (u32, u32)) {
        if self.whole {
            return;
        }
        let cut = |v: i32, max: u32| (v.max(0) as u32).min(max);
        let (x0, y0, x1, y1) = (cut(x0, w), cut(y0, h), cut(x1, w), cut(y1, h));
        if x1 > x0 && y1 > y0 {
            self.rects.add((x0, y0, x1 - x0, y1 - y0));
        }
    }

    /// Rows `[y0, y1)` changed, by a contributor that knows no columns (a widget loop, a text
    /// field's selection, a scroll layer's content): every column of a base `w` wide.
    pub(crate) fn rows(&mut self, (y0, y1): (i32, i32), w: u32) {
        self.add((0, y0, i32::try_from(w).unwrap_or(i32::MAX), y1), (w, u32::MAX));
    }

    /// Another change's damage too.
    pub(crate) fn merge(&mut self, other: Pending) {
        if other.whole {
            self.whole();
        } else if !self.whole {
            for r in other.rects.rects() {
                self.rects.add(*r);
            }
        }
    }

    /// Whether nothing is owed.
    pub(crate) fn is_empty(&self) -> bool {
        !self.whole && self.rects.is_empty()
    }

    /// What the present repaints, leaving nothing pending (`None`: nothing).
    pub(crate) fn take(&mut self) -> Option<Change> {
        let taken = core::mem::take(self);
        match taken {
            Pending { whole: true, .. } => Some(Change::Whole),
            Pending { rects, .. } if rects.is_empty() => None,
            Pending { rects, .. } => Some(Change::Rects(rects)),
        }
    }
}

/// How a trace line says it: `none`, `whole`, or the rectangles' count and pixels.
impl core::fmt::Display for Pending {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match (self.whole, self.rects.rects().len()) {
            (true, _) => f.write_str("whole"),
            (false, 0) => f.write_str("none"),
            (false, n) => write!(f, "rects(n={n} px={})", self.rects.px()),
        }
    }
}

/// Whether a scroll layer is covered (RFC-0109): an overlay drawn after the layer's root — never
/// its own content — PAINTS over its viewport (edges `x0, y0, x1, y1`). `painted`: each overlay's
/// node id and the extent its subtree paints (`motion_math::paint_extent`), so a full-bleed layer
/// that only places its content — a closed menu's host, a tap-catcher — covers nothing and the
/// band under it keeps scrolling; the sheet it holds, once open, does cover.
pub(crate) fn covered(
    root: usize,
    (vx0, vy0, vx1, vy1): (i32, i32, i32, i32),
    painted: &[(usize, (i32, i32, i32, i32))],
) -> bool {
    painted
        .iter()
        .any(|&(id, (x0, y0, x1, y1))| id > root && x0 < vx1 && vx0 < x1 && y0 < vy1 && vy0 < y1)
}

#[cfg(test)]
mod tests;
