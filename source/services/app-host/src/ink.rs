// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Where the base holds pixels — its INK (RFC-0121). A surface painted on a clear base
//! (the desktop shell, the greeter: `page_base_alpha() == 0`) tells the compositor where its
//! base has pixels, and the compositor draws the base only there: the wallpaper elsewhere is
//! not composed through a full-screen layer of clear pixels (about 10 ms of every full-screen
//! frame on the board).
//!
//! The ink is computed from what the base pass paints: every box of the pass where it paints
//! and its run where it prints (`layout_diff::paints_within` — the rectangles the diff names:
//! the drop shadow's and the text halo's reach, a stroke's caps, an ellipsis, cut to the box's
//! clip, a margin around); a box drawn through a scroll view (a covered layer, a flat one)
//! anywhere in that view's viewport, whatever its offset. Each rectangle is cut to the base and
//! merged: one that meets a rectangle already held takes it into their bounds (to a fixpoint —
//! the pieces stay disjoint, since the compositor blends each piece over the wallpaper and a
//! pixel in two pieces would be blended twice), and past the bound the pair whose bounds grow
//! the area least is merged. The area only ever grows: over-declaring costs fill, under-declaring
//! would cost pixels. A base box moved by an animation off identity paints outside its layout
//! place: the ink is the whole base (`INK_WHOLE`), as is a base that is not clear.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `ink_tests.rs` — the oracle `the_ink_holds_every_painted_pixel` (a shell-like
//!   and a greeter-like scene and seeded random ones painted by the real row painter, every pixel
//!   with alpha > 0 inside the ink), `test_reject_a_merge_that_drops_a_pixel`
//! INVARIANTS: every base pixel with alpha > 0 lies inside the ink; at most `MAX_INK` pairwise
//!   disjoint rectangles inside the base; no allocation (a fixed scratch).

use crate::layout_diff::TextRun;
use nexus_display_proto::surface_layers::{Rect16, SurfaceTree, INK_WHOLE, MAX_INK};
use nexus_layout::LayoutBox;

/// Edges `(x0, y0, x1, y1)` in base pixels.
type Edges = (i32, i32, i32, i32);

/// The rectangles the merge holds while it reads the boxes: past it, the cheapest pair merges.
/// Larger keeps more apart for the final merge to choose from; each box costs up to `CAP²/2`.
const CAP: usize = 24;
const _: () = assert!(CAP > MAX_INK);

/// The ink a structure carries: `rects[..count]`, or the whole base (`count == INK_WHOLE`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Ink {
    rects: [Rect16; MAX_INK],
    count: u8,
}

impl Ink {
    /// Not declared: the whole base (a window's, a base that is not clear, a base box moving).
    pub(crate) const WHOLE: Self = Self { rects: [Rect16::ZERO; MAX_INK], count: INK_WHOLE };

    /// The declared rectangles; `None` for the whole base.
    pub(crate) fn rects(&self) -> Option<&[Rect16]> {
        self.rects.get(..usize::from(self.count))
    }

    /// Written into the structure.
    pub(crate) fn put(&self, tree: &mut SurfaceTree) {
        tree.ink = self.rects;
        tree.ink_count = self.count;
    }

    /// Whether `tree` carries this ink.
    pub(crate) fn sent_in(&self, tree: &SurfaceTree) -> bool {
        tree.ink() == self.rects()
    }
}

/// Where the base pass draws a box.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Placed {
    /// Not in the base pass (a band's content, a motion layer's).
    Elsewhere,
    /// At its layout place.
    Here,
    /// Through a scroll view, inside its viewport `(x0, y0, x1, y1)` in base pixels.
    InView(Edges),
}

/// The ink of a base of `size` (`w`, `h`) over which `boxes` (pre-order) and their `texts` (by
/// node) are laid out: every box `placed` puts in the base pass, where it paints. `moved`: a
/// base box is animated off identity this frame — the whole base.
pub(crate) fn ink_of(
    boxes: &[LayoutBox],
    texts: &[TextRun],
    size: (u32, u32),
    moved: bool,
    placed: impl Fn(&LayoutBox) -> Placed,
) -> Ink {
    if moved {
        return Ink::WHOLE;
    }
    let (w, h) = (size.0.min(u32::from(u16::MAX)) as i32, size.1.min(u32::from(u16::MAX)) as i32);
    let mut set = Set::new();
    for b in boxes {
        let run = texts.binary_search_by_key(&b.node_id, |t| t.node).ok().map(|i| &texts[i]);
        let at = match placed(b) {
            Placed::Elsewhere => continue,
            Placed::Here => crate::layout_diff::paints_within(b, run),
            // Inside its viewport wherever the view is: an offset never changes the ink.
            Placed::InView(clip) => crate::layout_diff::paints_within(b, run).map(|_| clip),
        };
        if let Some(e) = at.and_then(|e| cut(e, (w, h))) {
            set.add(e, CAP);
        }
    }
    set.down_to(MAX_INK);
    set.ink()
}

/// `e` cut to the base `(0, 0, w, h)`; `None` when nothing is left.
fn cut(e: Edges, (w, h): (i32, i32)) -> Option<Edges> {
    let c = (e.0.max(0), e.1.max(0), e.2.min(w), e.3.min(h));
    (c.2 > c.0 && c.3 > c.1).then_some(c)
}

fn area(e: Edges) -> i64 {
    i64::from(e.2 - e.0) * i64::from(e.3 - e.1)
}

fn bounds(a: Edges, b: Edges) -> Edges {
    (a.0.min(b.0), a.1.min(b.1), a.2.max(b.2), a.3.max(b.3))
}

fn meets(a: Edges, b: Edges) -> bool {
    a.0 < b.2 && b.0 < a.2 && a.1 < b.3 && b.1 < a.3
}

fn holds(a: Edges, b: Edges) -> bool {
    a.0 <= b.0 && a.1 <= b.1 && a.2 >= b.2 && a.3 >= b.3
}

/// Pairwise disjoint rectangles, at most `CAP + 1` (one over while a merge is due).
struct Set {
    r: [Edges; CAP + 1],
    n: usize,
}

impl Set {
    const fn new() -> Self {
        Self { r: [(0, 0, 0, 0); CAP + 1], n: 0 }
    }

    /// One more painted rectangle, then merged down to `cap`.
    fn add(&mut self, e: Edges, cap: usize) {
        if self.r[..self.n].iter().any(|o| holds(*o, e)) {
            return;
        }
        self.absorb(e);
        self.down_to(cap);
    }

    /// `e` grown by every rectangle it meets, to a fixpoint (those leave), then held: the set
    /// stays disjoint, and only grows.
    fn absorb(&mut self, mut e: Edges) {
        loop {
            let mut grew = false;
            let mut i = 0;
            while i < self.n {
                if meets(self.r[i], e) {
                    e = bounds(e, self.r[i]);
                    self.n -= 1;
                    self.r[i] = self.r[self.n];
                    grew = true;
                } else {
                    i += 1;
                }
            }
            if !grew {
                break;
            }
        }
        self.r[self.n] = e;
        self.n += 1;
    }

    /// Merges the pair whose bounds grow the area least until at most `cap` are held.
    fn down_to(&mut self, cap: usize) {
        while self.n > cap.max(1) {
            let (mut best, mut pair) = (i64::MAX, (0, 1));
            for i in 0..self.n {
                for j in i + 1..self.n {
                    let (a, b) = (self.r[i], self.r[j]);
                    let growth = area(bounds(a, b)) - area(a) - area(b);
                    if growth < best {
                        (best, pair) = (growth, (i, j));
                    }
                }
            }
            let m = bounds(self.r[pair.0], self.r[pair.1]);
            // `j > i`: the later one leaves first, so the earlier index still names its own.
            for k in [pair.1, pair.0] {
                self.n -= 1;
                self.r[k] = self.r[self.n];
            }
            self.absorb(m);
        }
    }

    /// The held rectangles, top to bottom and left to right (stable keys for the same scene).
    fn ink(&mut self) -> Ink {
        if self.n > MAX_INK {
            return Ink::WHOLE; // never cut a rectangle off: the whole base instead
        }
        let held = &mut self.r[..self.n];
        held.sort_unstable_by_key(|e| (e.1, e.0));
        let mut ink = Ink { rects: [Rect16::ZERO; MAX_INK], count: held.len() as u8 };
        for (slot, e) in ink.rects.iter_mut().zip(held.iter()) {
            // Cut to the base before: every edge is within `0..=u16::MAX`.
            *slot = Rect16 {
                x: e.0 as u16,
                y: e.1 as u16,
                w: (e.2 - e.0) as u16,
                h: (e.3 - e.1) as u16,
            };
        }
        ink
    }
}

#[cfg(test)]
#[path = "ink_tests.rs"]
mod tests;
