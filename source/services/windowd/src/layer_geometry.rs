// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: a client surface's declared layers (RFC-0109) as windowd keeps them, and the pure
//! geometry its compositor composes from: a scroll layer's visible window cut into its pieces
//! (never a position its band does not hold), a wash placed in its parent and clipped to it, the
//! rows a present names for the copy, what a new structure changes on the screen, who may declare
//! layers on a surface. Host-tested here; the OS-only runtime (`compositor::runtime::
//! surface_layers`) keeps the state per surface and encodes it (`surface_layers_encode`).
//! windowd runs no scroll physics and decides no hover (ADR-0076).
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `mod tests` below

use nexus_display_proto::surface_layers::{
    self as proto, ScrollLayerDesc, SurfaceMotion, SurfaceTree, Wash, AXIS_VERTICAL,
};

/// A surface's declared layers.
#[derive(Clone, Copy)]
pub(crate) struct SurfaceLayers {
    /// The structure in effect.
    pub(crate) tree: SurfaceTree,
    /// A structure waiting for the surface's next present (the pixels it names).
    pending: Option<SurfaceTree>,
    /// The motion in effect.
    pub(crate) motion: SurfaceMotion,
    /// A refusal was said for this surface (once per surface: a broken client must not flood).
    refusal_said: bool,
}

/// What a present's structure changed on the screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TreeChange {
    /// Nothing.
    None,
    /// Only the content some bands hold (bit `k`: scroll layer `k`'s valid range).
    Bands(u8),
    /// Regions, viewports, bands or patches: the whole surface.
    Structure,
}

impl SurfaceLayers {
    pub(crate) const EMPTY: Self = Self {
        tree: SurfaceTree::EMPTY,
        pending: None,
        motion: SurfaceMotion {
            offsets: [0; proto::MAX_SCROLL_LAYERS],
            offset_count: 0,
            washes: [Wash { parent: 0, patch: 0, radius: 0, opacity: 0, x: 0, y: 0, w: 0, h: 0 };
                proto::MAX_WASHES],
            wash_count: 0,
        },
        refusal_said: false,
    };

    /// A new surface: nothing declared yet (fail-closed — no region, no band, no wash).
    pub(crate) fn clear(&mut self) {
        *self = Self::EMPTY;
    }

    /// The present: the waiting structure takes effect. Says what changed on the screen.
    pub(crate) fn activate(&mut self) -> TreeChange {
        let Some(next) = self.pending.take() else { return TreeChange::None };
        let change = structure_change(&self.tree, &next);
        self.tree = next;
        change
    }

    /// A structure for the surface's next present.
    pub(crate) fn stage(&mut self, tree: SurfaceTree) {
        self.pending = Some(tree);
    }

    /// The structure a motion is checked against: the one waiting for its present, else the one
    /// in effect (a motion may race the present of the structure it names).
    pub(crate) fn latest_tree(&self) -> SurfaceTree {
        self.pending.unwrap_or(self.tree)
    }

    /// Whether this is the surface's first refusal (said once: a broken client must not flood).
    pub(crate) fn first_refusal(&mut self) -> bool {
        !core::mem::replace(&mut self.refusal_said, true)
    }

    /// The motion's offset of scroll layer `k` (0 when the motion names none yet).
    pub(crate) fn offset(&self, k: usize) -> i32 {
        self.motion.offsets().get(k).copied().unwrap_or(0)
    }

    /// The most layers this declaration composes in a frame — its regions, its scroll layers'
    /// pieces at their offsets, its washes: the surface's demand on the frame's bound.
    pub(crate) fn demand(&self) -> usize {
        let mut out = [Piece::default(); MAX_PIECES];
        let slices: usize = (0..self.tree.scrolls().len())
            .map(|k| pieces(&self.tree.scrolls()[k], self.offset(k), &mut out))
            .sum();
        self.tree.regions().len() + slices + self.motion.washes().len()
    }
}

/// What `next` changes against `now`: only valid ranges → the bands; anything else → all.
fn structure_change(now: &SurfaceTree, next: &SurfaceTree) -> TreeChange {
    let same_but_valid = |a: &ScrollLayerDesc, b: &ScrollLayerDesc| {
        ScrollLayerDesc { valid_start: 0, valid_len: 0, ..*a }
            == ScrollLayerDesc { valid_start: 0, valid_len: 0, ..*b }
    };
    if now.regions() != next.regions()
        || now.patches() != next.patches()
        || now.scrolls().len() != next.scrolls().len()
        || now.scrolls().iter().zip(next.scrolls()).any(|(a, b)| !same_but_valid(a, b))
    {
        return TreeChange::Structure;
    }
    let mut bands = 0u8;
    for (k, (a, b)) in now.scrolls().iter().zip(next.scrolls()).enumerate() {
        if (a.valid_start, a.valid_len) != (b.valid_start, b.valid_len) {
            bands |= 1 << k;
        }
    }
    if bands == 0 {
        TreeChange::None
    } else {
        TreeChange::Bands(bands)
    }
}

/// One piece of a scroll layer's visible window: a rectangle of its band and where it lands, both
/// in surface coordinates.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Piece {
    pub(crate) src: (u32, u32),
    pub(crate) dst: (u32, u32),
    pub(crate) size: (u32, u32),
}

/// The most pieces a visible window takes: strips are at least half a viewport long, so a window
/// crosses at most two strip boundaries (the ring's wrap is one of them).
pub(crate) const MAX_PIECES: usize = 3;

/// The pieces of scroll layer `s` at `offset` (RFC-0109): the visible window
/// `[offset, offset + viewport)` intersected with the band's valid range, cut where a strip ends
/// (and where the ring wraps). Never a position the ring does not hold — the rest of the viewport
/// shows the base (the container's own background) until the band fills.
pub(crate) fn pieces(s: &ScrollLayerDesc, offset: i32, out: &mut [Piece; MAX_PIECES]) -> usize {
    let len = i64::from(s.viewport_len());
    let (valid0, valid1) =
        (i64::from(s.valid_start), i64::from(s.valid_start) + i64::from(s.valid_len));
    let o = i64::from(offset);
    let c0 = o.max(valid0).max(0);
    let c1 = (o + len).min(valid1);
    let mut p = c0;
    let mut n = 0;
    while p < c1 && n < MAX_PIECES {
        let Some((x, y, run)) = s.ring_place(p as u32) else { break };
        let take = (c1 - p).min(i64::from(run)) as u32;
        let along = (p - o) as u32;
        out[n] = if s.axis == AXIS_VERTICAL {
            Piece {
                src: (x, y),
                dst: (u32::from(s.vx), u32::from(s.vy) + along),
                size: (u32::from(s.vw), take),
            }
        } else {
            Piece {
                src: (x, y),
                dst: (u32::from(s.vx) + along, u32::from(s.vy)),
                size: (take, u32::from(s.vh)),
            }
        };
        n += 1;
        p += i64::from(take);
    }
    n
}

/// Where wash `w` lands in surface coordinates `(x, y, w, h)`: its parent's content coordinates
/// moved by the parent's offset and clipped to the parent's viewport (the base: the base layer).
/// `None` when nothing of it is visible or its parent is not declared.
pub(crate) fn wash_place(w: &Wash, layers: &SurfaceLayers, base: (u32, u32)) -> Option<Piece> {
    let (x, y) = (i64::from(w.x), i64::from(w.y));
    let (rect, clip) = match usize::from(w.parent) {
        0 => ((x, y), (0, 0, i64::from(base.0), i64::from(base.1))),
        k => {
            let s = layers.tree.scrolls().get(k - 1)?;
            let o = i64::from(layers.offset(k - 1));
            let (vx, vy) = (i64::from(s.vx), i64::from(s.vy));
            let at =
                if s.axis == AXIS_VERTICAL { (vx + x, vy + y - o) } else { (vx + x - o, vy + y) };
            (at, (vx, vy, i64::from(s.vw), i64::from(s.vh)))
        }
    };
    let x0 = rect.0.max(clip.0);
    let y0 = rect.1.max(clip.1);
    let x1 = (rect.0 + i64::from(w.w)).min(clip.0 + clip.2);
    let y1 = (rect.1 + i64::from(w.h)).min(clip.1 + clip.3);
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    let patch = layers.tree.patches().get(usize::from(w.patch))?;
    Some(Piece {
        src: (u32::from(patch.x) + 1, u32::from(patch.y) + 1),
        dst: (x0 as u32, y0 as u32),
        size: ((x1 - x0) as u32, (y1 - y0) as u32),
    })
}

/// The rows of a surface a present names, waiting for the copy into the atlas: up to four spans
/// (a present names at most four rectangles), overlapping spans merged; `full` = every row.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct DamageRows {
    spans: [(u32, u32); 4],
    n: usize,
    pub(crate) full: bool,
}

impl DamageRows {
    pub(crate) const FULL: Self = Self { spans: [(0, 0); 4], n: 0, full: true };
    pub(crate) const NONE: Self = Self { spans: [(0, 0); 4], n: 0, full: false };

    /// Rows `[y0, y1)` to copy. A fifth span joins the span nearest to it (copying a row twice
    /// is harmless, losing one is not).
    pub(crate) fn add(&mut self, y0: u32, y1: u32) {
        if self.full || y1 <= y0 {
            return;
        }
        let (mut a, mut b) = (y0, y1);
        // Absorb every span it touches.
        let mut i = 0;
        while i < self.n {
            let (s0, s1) = self.spans[i];
            if s0 <= b && a <= s1 {
                a = a.min(s0);
                b = b.max(s1);
                self.spans[i] = self.spans[self.n - 1];
                self.n -= 1;
            } else {
                i += 1;
            }
        }
        if self.n < self.spans.len() {
            self.spans[self.n] = (a, b);
            self.n += 1;
            return;
        }
        let gap = |s: (u32, u32)| if s.1 < a { a - s.1 } else { s.0.saturating_sub(b) };
        let near = (0..self.n).min_by_key(|&k| gap(self.spans[k])).unwrap_or(0);
        let s = self.spans[near];
        self.spans[near] = (s.0.min(a), s.1.max(b));
    }

    /// The spans to copy, each clamped to `rows` (every row when `full`).
    pub(crate) fn spans(&self, rows: u32, out: &mut [(u32, u32); 4]) -> usize {
        if self.full {
            out[0] = (0, rows);
            return 1;
        }
        let mut n = 0;
        for &(a, b) in &self.spans[..self.n] {
            let (a, b) = (a.min(rows), b.min(rows));
            if a < b {
                out[n] = (a, b);
                n += 1;
            }
        }
        n
    }
}

/// Whether a frame's sender owns the surface it names: the kernel's sender id equals the one
/// captured at the surface's create, and the surface had a named creator at all (an unnamed
/// spawn's 0 owns nothing).
pub(crate) fn owns(owner_sid: u64, sender_sid: u64) -> bool {
    owner_sid != 0 && owner_sid == sender_sid
}

/// The screen damage of a present's rectangle in the surface's STORE (a band's rows): the
/// viewports of the scroll layers whose band it touches, in surface coordinates. Patches
/// change nothing on the screen by themselves (a wash using one is motion).
pub(crate) fn store_damage(
    layers: &SurfaceLayers,
    rect: (u32, u32, u32, u32),
    out: &mut [(u32, u32, u32, u32); proto::MAX_SCROLL_LAYERS],
) -> usize {
    let mut n = 0;
    for s in layers.tree.scrolls() {
        let b = s.band_rect();
        let touches = rect.0 < b.0 + b.2
            && b.0 < rect.0 + rect.2
            && rect.1 < b.1 + b.3
            && b.1 < rect.1 + rect.3;
        if touches && s.valid_len > 0 {
            out[n] = (u32::from(s.vx), u32::from(s.vy), u32::from(s.vw), u32::from(s.vh));
            n += 1;
        }
    }
    n
}

/// Max-min fair shares of the frame's layers (RFC-0109 failure model): `demand[i]` is what
/// surface `i` declares, `rest` what the planner's set holds beside the bodies. When everything
/// fits each surface keeps its demand; otherwise the smaller demands stay whole and the rest is
/// split evenly among the larger — a surface over the bound loses its own last layers, never
/// another window's.
pub(crate) fn fair_shares(rest: usize, demand: &[usize], share: &mut [usize]) {
    let n = demand.len().min(share.len());
    // `usize::MAX` marks a surface whose share is still open.
    share[..n].fill(usize::MAX);
    let (mut left, mut open) = (rest, n);
    while open > 0 {
        let even = left / open;
        let smallest = (0..n)
            .filter(|&i| share[i] == usize::MAX)
            .min_by_key(|&i| demand[i])
            .filter(|&i| demand[i] <= even);
        let Some(i) = smallest else {
            // Every open demand exceeds the even split: each takes it, the remainder in order.
            let mut extra = left - even * open;
            for s in share[..n].iter_mut().filter(|s| **s == usize::MAX) {
                *s = even + usize::from(extra > 0);
                extra = extra.saturating_sub(1);
            }
            return;
        };
        share[i] = demand[i];
        left -= demand[i];
        open -= 1;
    }
}

/// One surface's share of the frame's layers as its encode spends it: a layer past the share is
/// not composed, and counted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LayerQuota {
    left: usize,
    dropped: usize,
}

impl LayerQuota {
    pub(crate) const fn new(share: usize) -> Self {
        Self { left: share, dropped: 0 }
    }

    /// Whether one more layer fits the share (counted as dropped when it does not).
    pub(crate) fn take(&mut self) -> bool {
        if self.left == 0 {
            self.dropped += 1;
            return false;
        }
        self.left -= 1;
        true
    }

    /// The layers the surface lost to its share this frame.
    pub(crate) const fn dropped(&self) -> usize {
        self.dropped
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_display_proto::surface_layers::{Patch, AXIS_HORIZONTAL};

    fn vertical() -> ScrollLayerDesc {
        ScrollLayerDesc {
            vx: 310,
            vy: 120,
            vw: 1080,
            vh: 700,
            axis: AXIS_VERTICAL,
            flags: 0,
            bx: 0,
            by: 869,
            strip_len: 1400,
            strips: 1,
            valid_start: 0,
            valid_len: 1400,
        }
    }

    #[test]
    fn the_visible_window_is_one_piece_inside_the_ring() {
        let mut out = [Piece::default(); MAX_PIECES];
        let n = pieces(&vertical(), 300, &mut out);
        assert_eq!(n, 1);
        assert_eq!(out[0], Piece { src: (0, 869 + 300), dst: (310, 120), size: (1080, 700) });
    }

    #[test]
    fn the_window_across_the_ring_wrap_is_two_pieces() {
        let mut s = vertical();
        s.valid_start = 1000;
        let mut out = [Piece::default(); MAX_PIECES];
        // Content 1100..1800 sits at ring rows 1100..1400 and 0..400.
        let n = pieces(&s, 1100, &mut out);
        assert_eq!(n, 2);
        assert_eq!(out[0], Piece { src: (0, 869 + 1100), dst: (310, 120), size: (1080, 300) });
        assert_eq!(out[1], Piece { src: (0, 869), dst: (310, 420), size: (1080, 400) });
    }

    /// The band behind the viewport: only what the ring holds is drawn — never an evicted range.
    #[test]
    fn test_reject_showing_content_outside_the_valid_range() {
        let mut s = vertical();
        s.valid_start = 0;
        s.valid_len = 900;
        let mut out = [Piece::default(); MAX_PIECES];
        let n = pieces(&s, 500, &mut out);
        assert_eq!(n, 1);
        assert_eq!(out[0].size, (1080, 400), "content 500..900 only");
        s.valid_len = 0; // covered
        assert_eq!(pieces(&s, 500, &mut out), 0);
        s.valid_start = 5000;
        s.valid_len = 1400; // the ring moved on: nothing of 500..1200 is there
        assert_eq!(pieces(&s, 500, &mut out), 0);
    }

    #[test]
    fn a_folded_horizontal_ring_cuts_at_its_strips() {
        let s = ScrollLayerDesc {
            vx: 0,
            vy: 100,
            vw: 1920,
            vh: 600,
            axis: AXIS_HORIZONTAL,
            flags: 0,
            bx: 0,
            by: 1096,
            strip_len: 1920,
            strips: 2,
            valid_start: 0,
            valid_len: 3840,
        };
        let mut out = [Piece::default(); MAX_PIECES];
        let n = pieces(&s, 1000, &mut out);
        assert_eq!(n, 2);
        assert_eq!(out[0], Piece { src: (1000, 1096), dst: (0, 100), size: (920, 600) });
        assert_eq!(out[1], Piece { src: (0, 1696), dst: (920, 100), size: (1000, 600) });
    }

    #[test]
    fn a_wash_follows_its_scroll_layer_and_is_clipped_to_its_viewport() {
        let mut layers = SurfaceLayers::EMPTY;
        layers.tree.scrolls[0] = vertical();
        layers.tree.scroll_count = 1;
        layers.tree.patches[0] = Patch { x: 1400, y: 853 };
        layers.tree.patch_count = 1;
        layers.motion.offsets[0] = 400;
        layers.motion.offset_count = 1;
        // A row at content y 420: on the surface at 120 + 420 - 400 = 140.
        let w = Wash { parent: 1, patch: 0, radius: 10, opacity: 38, x: 0, y: 420, w: 1080, h: 41 };
        let p = wash_place(&w, &layers, (1440, 853)).expect("visible");
        assert_eq!(p, Piece { src: (1401, 854), dst: (310, 140), size: (1080, 41) });
        // A row half above the viewport: cut at its top.
        let w = Wash { y: 390, ..w };
        let p = wash_place(&w, &layers, (1440, 853)).expect("visible");
        assert_eq!((p.dst, p.size), ((310, 120), (1080, 31)));
        // A row scrolled out: nothing.
        let w = Wash { y: 100, ..w };
        assert_eq!(wash_place(&w, &layers, (1440, 853)), None);
        // A wash on the base: base coordinates.
        let w = Wash { parent: 0, x: 20, y: 30, w: 200, h: 44, ..w };
        assert_eq!(wash_place(&w, &layers, (1440, 853)).map(|p| p.dst), Some((20, 30)));
    }

    #[test]
    fn damage_rows_merge_and_stay_bounded() {
        let mut d = DamageRows::NONE;
        d.add(10, 20);
        d.add(15, 30);
        d.add(900, 964);
        d.add(1200, 1300);
        d.add(2000, 2100);
        d.add(2500, 2600); // a fifth: joins its nearest
        let mut out = [(0, 0); 4];
        let n = d.spans(3000, &mut out);
        assert_eq!(n, 4);
        assert!(out[..n].contains(&(10, 30)));
        assert!(out[..n].iter().any(|s| s.0 <= 2000 && s.1 >= 2600), "{out:?}");
        let n = DamageRows::FULL.spans(853, &mut out);
        assert_eq!(&out[..n], &[(0, 853)]);
    }

    /// RFC-0109: a structure takes effect with the present of the pixels it names — never before;
    /// a motion is checked against the structure about to be in effect; a refusal is said once.
    #[test]
    fn a_structure_takes_effect_with_its_present() {
        let mut layers = SurfaceLayers::EMPTY;
        let mut tree = SurfaceTree::EMPTY;
        tree.scrolls[0] = vertical();
        tree.scroll_count = 1;
        layers.stage(tree);
        assert_eq!(layers.latest_tree(), tree);
        assert_eq!(layers.tree, SurfaceTree::EMPTY, "not before its present");
        assert_eq!(layers.activate(), TreeChange::Structure);
        assert_eq!(layers.tree, tree);
        assert_eq!(layers.activate(), TreeChange::None, "nothing waiting");
        assert!(layers.first_refusal());
        assert!(!layers.first_refusal(), "said once");
        layers.clear();
        assert_eq!(layers.tree, SurfaceTree::EMPTY);
        assert!(layers.first_refusal(), "a new surface may be refused again");
    }

    /// A band's new rows show in its scroll layer's viewport; a patch's in nothing.
    #[test]
    fn store_rows_damage_the_viewport_of_their_band() {
        let mut layers = SurfaceLayers::EMPTY;
        layers.tree.scrolls[0] = vertical();
        layers.tree.scroll_count = 1;
        let mut out = [(0, 0, 0, 0); proto::MAX_SCROLL_LAYERS];
        assert_eq!(store_damage(&layers, (0, 1000, 1080, 64), &mut out), 1);
        assert_eq!(out[0], (310, 120, 1080, 700));
        assert_eq!(store_damage(&layers, (1400, 853, 16, 16), &mut out), 0, "a patch");
        layers.tree.scrolls[0].valid_len = 0; // covered: nothing of the band shows
        assert_eq!(store_damage(&layers, (0, 1000, 1080, 64), &mut out), 0);
    }

    /// RFC-0109: a client declares layers on its own surface only.
    #[test]
    fn test_reject_layers_for_a_surface_the_sender_does_not_own() {
        assert!(owns(0x51, 0x51));
        assert!(!owns(0x51, 0x52), "another client's surface");
        assert!(!owns(0, 0), "an unnamed creator owns nothing");
    }

    #[test]
    fn a_new_valid_range_changes_its_band_alone() {
        let mut now = SurfaceTree::EMPTY;
        now.scrolls[0] = vertical();
        now.scroll_count = 1;
        let mut next = now;
        next.scrolls[0].valid_start = 64;
        assert_eq!(structure_change(&now, &next), TreeChange::Bands(1));
        let mut moved = now;
        moved.scrolls[0].vy = 130;
        assert_eq!(structure_change(&now, &moved), TreeChange::Structure);
        assert_eq!(structure_change(&now, &now), TreeChange::None);
    }

    /// A heavy surface beside light ones keeps every layer while the frame holds them all.
    #[test]
    fn every_declared_layer_is_composed_while_the_frame_holds_them() {
        let mut share = [0; 5];
        fair_shares(58, &[32, 3, 3, 3, 0], &mut share);
        assert_eq!(share, [32, 3, 3, 3, 0]);
        let mut layers = SurfaceLayers::EMPTY;
        layers.tree.scrolls[0] = vertical();
        layers.tree.scroll_count = 1;
        assert_eq!(layers.demand(), 1, "one scroll layer, its window in one piece");
    }

    /// RFC-0109: past the planner's set the greediest surfaces lose their own last layers; a
    /// surface declaring little keeps everything, and the frame stays inside the bound.
    #[test]
    fn test_reject_a_surface_crowding_out_another_window() {
        let mut share = [0; 5];
        fair_shares(58, &[32, 32, 32, 3, 3], &mut share);
        assert_eq!(&share[3..], &[3, 3], "the light windows keep theirs");
        assert_eq!(share[..3].iter().sum::<usize>(), 52);
        assert!(share[..3].iter().all(|&s| s == 17 || s == 18), "split evenly: {share:?}");
        let mut quota = LayerQuota::new(share[0]);
        let kept = (0..32).filter(|_| quota.take()).count();
        assert_eq!((kept, quota.dropped()), (share[0], 32 - share[0]));
    }
}
