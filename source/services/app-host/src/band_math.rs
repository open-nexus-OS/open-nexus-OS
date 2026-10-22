// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the pure decisions of a scroll layer's virtual band (RFC-0109): what it draws next —
//! the visible window first, below then above, then the margins ahead and behind, starting over
//! at the window when it holds nothing of it — how its valid range grows as the ring overwrites
//! what it held, and the bounded list of rectangles a present names. Same one-cfg shape as
//! `hover_wash`: the drawing lives in RISC-V-only `probe/band.rs`, the decisions here where host
//! tests reach them.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: unit tests below

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

/// What a present names: up to four surface rectangles (the wire's bound), a fifth merged into
/// the one it grows least.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct PresentDamage {
    rects: [(u32, u32, u32, u32); 4],
    n: usize,
}

impl PresentDamage {
    pub(crate) fn add(&mut self, r: (u32, u32, u32, u32)) {
        if r.2 == 0 || r.3 == 0 {
            return;
        }
        let union = |a: (u32, u32, u32, u32), b: (u32, u32, u32, u32)| {
            let (x0, y0) = (a.0.min(b.0), a.1.min(b.1));
            let (x1, y1) = ((a.0 + a.2).max(b.0 + b.2), (a.1 + a.3).max(b.1 + b.3));
            (x0, y0, x1 - x0, y1 - y0)
        };
        if self.n < self.rects.len() {
            self.rects[self.n] = r;
            self.n += 1;
            return;
        }
        let area = |q: (u32, u32, u32, u32)| u64::from(q.2) * u64::from(q.3);
        let k = (0..self.n)
            .min_by_key(|&k| area(union(self.rects[k], r)) - area(self.rects[k]))
            .unwrap_or(0);
        self.rects[k] = union(self.rects[k], r);
    }

    pub(crate) fn rects(&self) -> &[(u32, u32, u32, u32)] {
        &self.rects[..self.n]
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.n == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 1400-position ring for a 700-row viewport.
    fn band() -> Band {
        Band { bx: 0, by: 869, strip_len: 1400, strips: 1, valid_start: 0, valid_len: 0 }
    }

    #[test]
    fn an_empty_band_draws_its_visible_window_first() {
        let f = next_fill(&band(), 700, 5000, 300, false).expect("a fill");
        assert_eq!((f.from, f.to, f.visible, f.restart), (300, 1000, true, true));
    }

    #[test]
    fn then_the_margins_ahead_and_behind() {
        let mut b = band();
        grow(&mut b, 300, 1000);
        let ahead = next_fill(&b, 700, 5000, 300, false).expect("ahead");
        assert_eq!((ahead.from, ahead.to, ahead.visible, ahead.forward), (1000, 1350, false, true));
        grow(&mut b, 1000, 1350);
        let behind = next_fill(&b, 700, 5000, 300, false).expect("behind");
        assert_eq!((behind.from, behind.to, behind.forward), (0, 300, false));
        grow(&mut b, 0, 300);
        assert_eq!(next_fill(&b, 700, 5000, 300, false), None, "the target is held");
        assert_eq!((b.valid_start, b.valid_len), (0, 1350));
    }

    /// Scrolling on: the visible window's new rows come before any margin, and the ring drops
    /// what it overwrites at the far end.
    #[test]
    fn scrolling_fills_the_window_first_and_the_ring_forgets_behind() {
        let mut b = band();
        grow(&mut b, 0, 1050);
        let f = next_fill(&b, 700, 5000, 400, false).expect("the window's gap");
        assert_eq!((f.from, f.to, f.visible), (1050, 1100, true));
        grow(&mut b, 1050, 1100);
        let f = next_fill(&b, 700, 5000, 400, false).expect("the margin");
        assert!(!f.visible && f.forward);
        grow(&mut b, 1100, 1450);
        assert_eq!(
            (b.valid_start, b.valid_end()),
            (50, 1450),
            "1400 held, the first 50 overwritten"
        );
    }

    /// A jump past everything held starts over at the new window — never a stale range shown.
    #[test]
    fn test_reject_keeping_a_range_the_window_left() {
        let mut b = band();
        grow(&mut b, 0, 1400);
        let f = next_fill(&b, 700, 9000, 6000, false).expect("restart");
        assert!(f.restart && f.visible);
        assert_eq!((f.from, f.to), (6000, 6700));
    }

    #[test]
    fn a_covered_or_empty_layer_draws_nothing() {
        assert_eq!(next_fill(&band(), 700, 5000, 0, true), None);
        assert_eq!(next_fill(&band(), 700, 0, 0, false), None);
        assert_eq!(next_fill(&Band::default(), 700, 5000, 0, false), None);
    }

    #[test]
    fn a_run_never_crosses_a_strip_end() {
        // Two 960-column strips: positions 900..1100 cross the first strip's end at 960.
        let f = Fill { from: 900, to: 1100, visible: true, forward: true, restart: false };
        assert_eq!(run_of(&f, 960, 1000), (900, 960));
        let back = Fill { from: 900, to: 1100, visible: false, forward: false, restart: false };
        assert_eq!(run_of(&back, 960, 1000), (960, 1100), "backward: the run ends at `to`");
        assert_eq!(run_of(&back, 960, 64), (1036, 1100), "a chunk at a time");
    }

    #[test]
    fn a_backward_run_trims_the_far_end_the_ring_overwrote() {
        let mut b = band();
        grow(&mut b, 600, 2000);
        grow(&mut b, 500, 600);
        assert_eq!((b.valid_start, b.valid_end()), (500, 1900));
    }

    #[test]
    fn present_damage_stays_within_four_rectangles() {
        let mut d = PresentDamage::default();
        assert!(d.is_empty());
        for k in 0..6u32 {
            d.add((0, k * 100, 1440, 10));
        }
        assert_eq!(d.rects().len(), 4);
        let rows: u32 = d.rects().iter().map(|r| r.3).sum();
        assert!(rows >= 60, "every named row is still named");
        d.add((0, 0, 0, 10));
        assert_eq!(d.rects().len(), 4, "an empty rectangle names nothing");
    }
}
