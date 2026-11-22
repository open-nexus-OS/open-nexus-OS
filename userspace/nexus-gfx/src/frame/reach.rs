// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Which of a frame's changes reach a glass (TASK-0351 P6, RFC-0101 §8). A present
//! names what changed as rectangles, each with its origin — the KEY of the layer that draws the
//! change (RFC-0109; `0` = unknown or the base, `ORIGIN_ABOVE` = the pointer). The plan's walk
//! asks here, glass by glass, which changes alter the glass's backdrop:
//! R1 a change belongs to the step whose key it names — it is the backdrop of the glasses above
//!    that step only, never of the step itself, of a glass below it or of a glass of the same
//!    window; a key the set does not hold is as origin 0: below every glass.
//! R2 a change ends at a cover — where the opaque rectangles of the steps between its own and
//!    the glass hold every pixel of it within the glass's reach, the glass's backdrop is those
//!    steps' pixels and did not change. Never asked for an unknown origin: its layer may lie
//!    above the cover. Not asked for a glass held while the layers below it are displaced
//!    (`Changes::uncovered`, P19-e): the covers that stand now need not stand at rest.
//! R3 a glass blurred again is a change of its own step for the glasses above it.
//! R4 layers that swapped their order changed the screen where they overlap, though neither is
//!    drawn otherwise.
//! OWNERS: @gpu @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! The writes are counted by what reached the glass (`by`: `WritesBy`, an unknown origin split
//! into `zero` and `stray`, the key misses at rest by what differs — counters, TASK-0351 P19).
//! TEST_COVERAGE: unit tests below; `tests/frame_reach.rs` (the rules' write counts),
//!   `tests/frame_reach_oracle.rs` (a planned frame against a full redraw, 200 presents),
//!   `tests/frame_rest_misses.rs` (the counters through a plan), `tests/frame_hold_owe.rs` (the
//!   uncovered owe of a held glass against pixels)
//! INVARIANTS: a change is kept from a glass only by a proof (its step is the glass or above it,
//!   or a cover holds it) — an unknown origin and an uncertain cover always reach; the list is
//!   the plan's own storage (reserved once a plan, never grown in the walk), and a change it
//!   could not hold makes every glass blur whole; a key is searched for, never an index.

use alloc::vec::Vec;

use super::damage::Area;
use super::glass::CacheStep;
use super::layers::{FrameLayer, RetainedLayers, MAX_DAMAGE, ORIGIN_ABOVE};
use super::occlude::{self, Px};
use super::plan::LayerStep;

mod by;

pub(super) use by::OweFull;
pub use by::{OweWhole, RestMisses, RestWhole, WritesBy};

/// How far a glass's blur reads beyond its rectangle: the widest blur a layer may ask (64, the
/// buffer's bound) and the chain's own margin.
pub(super) const GLASS_REACH: u32 = 66;

/// An area as a rectangle in pixels; `None` for no area and for the whole screen.
pub(super) fn px(a: Area) -> Option<Px> {
    match a {
        Area::Rect { x0, y0, x1, y1 } => Some((x0, y0, x1 - x0, y1 - y0)),
        _ => None,
    }
}

/// Where a change lies in the walk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Origin {
    /// Origin 0: the base or an unknown layer — below every glass, behind no cover.
    Zero,
    /// A key the set does not hold: as `Zero`, and never a desktop region's own content.
    Stray,
    /// The content of step `i` (R1).
    Layer(usize),
    /// Glass `i`, blurred again (R3).
    Glass(usize),
    /// Layers swapped their order, the lowest of them step `i` (R4).
    Order(usize),
    /// Above every layer (the pointer).
    Above,
}

/// One change of the frame.
#[derive(Clone, Copy, Debug)]
struct Change {
    area: Area,
    from: Origin,
}

/// A glass as the walk reaches it: its step, its rectangle and its window's transform id.
#[derive(Clone, Copy, Debug)]
pub(super) struct GlassAt {
    pub(super) index: usize,
    pub(super) rect: Area,
    pub(super) id: u32,
}

/// What reaches a glass this frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Touch {
    /// The changes below the glass within its reach, clipped to it: the band it blurs again.
    pub(super) band: Area,
    /// A change of origin 0 is among them (`Writes::band_base`).
    pub(super) base: bool,
    /// A change of a key the set does not hold is among them.
    stray: bool,
    below: bool,
    glass: bool,
    order: bool,
}

impl Touch {
    /// Nothing reaches the glass.
    const NONE: Self = Self {
        band: Area::None,
        base: false,
        stray: false,
        below: false,
        glass: false,
        order: false,
    };

    /// A full present: everything blurs again, whatever was named.
    pub(super) const FULL: Self = Self { band: Area::Full, ..Self::NONE };
}

/// Whether the opaque rectangles of the steps above `from` hold every pixel of `area`: a
/// layer flagged opaque, or a glass whose blurred back is drawn from its rows this frame.
fn covered(area: Area, from: usize, steps: &[LayerStep], covers: &mut Vec<Px>) -> bool {
    let Some(q) = px(area) else { return false };
    covers.clear();
    for s in steps.iter().skip(from + 1) {
        if covers.len() == covers.capacity() {
            return false;
        }
        let back =
            matches!(s.glass.map(|g| g.cache), Some(CacheStep::Read(_) | CacheStep::Write(_)));
        covers.extend(occlude::opaque_rect(&s.layer, back).filter(|o| occlude::intersects(*o, q)));
    }
    !covers.is_empty() && occlude::covered(q, covers)
}

/// A frame's changes: the present's named rectangles, the overlap of the layers that swapped
/// their order, and every glass blurred again so far in the walk. The plan's own storage.
pub(super) struct Changes {
    list: Vec<Change>,
    /// A change found no room: every glass blurs whole this frame.
    lost: bool,
}

impl Changes {
    pub(super) const fn new() -> Self {
        Self { list: Vec::new(), lost: false }
    }

    /// Emptied, with room for a present's rectangles, one reorder and a write of each of
    /// `layers` steps; `false` when the heap refuses it.
    pub(super) fn begin(&mut self, layers: usize) -> bool {
        self.list.clear();
        self.lost = false;
        self.list.try_reserve(MAX_DAMAGE + 1 + layers).is_ok()
    }

    fn push(&mut self, c: Change) {
        if c.area == Area::None {
            return;
        }
        if self.list.len() < self.list.capacity() {
            self.list.push(c);
        } else {
            self.lost = true;
        }
    }

    /// The present's rectangles, each at the step its origin names (R1): the first layer of
    /// that key in the set (`drawn`: the set as drawn).
    pub(super) fn name(
        &mut self,
        named: &[(Area, u32)],
        layers: &RetainedLayers,
        drawn: &[FrameLayer],
    ) {
        for &(area, origin) in named {
            let from = match origin {
                ORIGIN_ABOVE => Origin::Above,
                0 => Origin::Zero,
                key => match layers.index_of_key(key).filter(|i| *i < drawn.len()) {
                    Some(i) => Origin::Layer(i),
                    None => Origin::Stray,
                },
            };
            self.push(Change { area, from });
        }
    }

    /// R4: where layers that swapped their order overlap — `carried[i]` is the last plan's
    /// step layer `i` carries on from, so an order kept is an ascending list (one pass; the
    /// pairs are looked at only otherwise). The overlaps' bounds, recorded as one change of the
    /// lowest such layer and returned for the frame's damage.
    pub(super) fn reorder(
        &mut self,
        carried: &[Option<usize>],
        drawn: &[FrameLayer],
        extent: fn(&FrameLayer) -> Area,
    ) -> Area {
        let mut kept = carried.iter().flatten();
        let Some(mut last) = kept.next() else { return Area::None };
        let ascending = kept.all(|c| core::mem::replace(&mut last, c) < c);
        if ascending {
            return Area::None;
        }
        let (mut area, mut lowest) = (Area::None, None);
        for (p, (was_p, lp)) in carried.iter().zip(drawn).enumerate() {
            let Some(was_p) = was_p else { continue };
            let below = extent(lp);
            for (was_q, lq) in carried.iter().zip(drawn).skip(p + 1) {
                if was_q.is_some_and(|w| w < *was_p) {
                    let both = below.intersect(extent(lq));
                    if both != Area::None {
                        area = area.union(both);
                        lowest.get_or_insert(p);
                    }
                }
            }
        }
        if let Some(p) = lowest {
            self.push(Change { area, from: Origin::Order(p) });
        }
        area
    }

    /// R3: glass `index` was blurred again and shows it in `area`.
    pub(super) fn written(&mut self, index: usize, area: Area) {
        self.push(Change { area, from: Origin::Glass(index) });
    }

    /// What reaches glass `g`. `steps`: the plan's steps below it, as decided this frame;
    /// `covers`: scratch with room for a rectangle of each.
    pub(super) fn touch(&self, g: &GlassAt, steps: &[LayerStep], covers: &mut Vec<Px>) -> Touch {
        self.reaching(g, steps, Some(covers))
    }

    /// What reaches glass `g` with no change ended at a cover (P19-e) — asked for a glass held
    /// while the layers below it are displaced: a change is named where its layer is drawn
    /// now, and a layer that does not move rests there, but the covers above it may move. A
    /// layer that moves is paid where it rests by the displacement (`Moves::below`).
    pub(super) fn uncovered(&self, g: &GlassAt, steps: &[LayerStep]) -> Touch {
        self.reaching(g, steps, None)
    }

    /// `touch`, a change ended at a cover only when `covers` (scratch) is given (R2).
    fn reaching(
        &self,
        g: &GlassAt,
        steps: &[LayerStep],
        mut covers: Option<&mut Vec<Px>>,
    ) -> Touch {
        if self.lost {
            return Touch::FULL;
        }
        let reach = g.rect.grown(GLASS_REACH);
        let id_of = |i: usize| steps.get(i).map(|s| s.layer.layer_id);
        let mut t = Touch::NONE;
        for c in self.list.iter().filter(|c| c.area.touches(reach)) {
            let within = c.area.intersect(reach);
            let mut ends = |i: usize| covers.as_mut().is_some_and(|v| covered(within, i, steps, v));
            let hit = match c.from {
                Origin::Above => false,
                // The desktop's own glass regions (id 0: the dock's buttons, the bar's pills)
                // draw the desktop surface's pixels over a blur of the same surface: a change
                // of the desktop within such a region is the region's own content, not its
                // backdrop (cycle 38: those regions were blurred again every frame).
                Origin::Zero => !(g.id == 0 && c.area.inside(g.rect)),
                Origin::Stray => true,
                // Every slice of a window shares its id: a window's own change is never below
                // one of its own glasses (cycle 41) — and the desktop's layers (id 0) are the
                // desktop surface of the rule above.
                Origin::Layer(i) => {
                    i < g.index
                        && id_of(i)
                            .is_some_and(|id| !(id == g.id && (id != 0 || c.area.inside(g.rect))))
                        && !ends(i)
                }
                Origin::Glass(i) => {
                    i < g.index && id_of(i).is_some_and(|id| id == 0 || id != g.id) && !ends(i)
                }
                Origin::Order(i) => i < g.index,
            };
            if !hit {
                continue;
            }
            t.band = t.band.union(within);
            match c.from {
                Origin::Zero => t.base = true,
                Origin::Stray => t.stray = true,
                Origin::Layer(_) => t.below = true,
                Origin::Glass(_) => t.glass = true,
                Origin::Order(_) => t.order = true,
                Origin::Above => {}
            }
        }
        t
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::plan::Glass;

    fn step(x: u32, y: u32, w: u32, h: u32, f: impl FnOnce(&mut FrameLayer)) -> LayerStep {
        let mut layer = FrameLayer { opacity: 255, ..FrameLayer::EMPTY };
        (layer.dst_x, layer.dst_y, layer.width, layer.height) = (x, y, w, h);
        f(&mut layer);
        LayerStep { layer, glass: None, upload: false }
    }

    fn changes(list: &[(Area, Origin)]) -> Changes {
        let mut c = Changes::new();
        assert!(c.begin(8));
        for &(area, from) in list {
            c.push(Change { area, from });
        }
        c
    }

    const GLASS: GlassAt =
        GlassAt { index: 2, rect: Area::Rect { x0: 300, y0: 300, x1: 700, y1: 500 }, id: 7 };

    fn touched(c: &Changes, steps: &[LayerStep]) -> Touch {
        let mut covers = Vec::with_capacity(steps.len());
        c.touch(&GLASS, steps, &mut covers)
    }

    #[test]
    fn a_change_reaches_the_glasses_above_its_step_and_no_other() {
        let steps = [step(0, 0, 1920, 1080, |_| {}), step(200, 200, 600, 400, |l| l.layer_id = 3)];
        let r = Area::rect(350, 350, 20, 10);
        assert_eq!(touched(&changes(&[(r, Origin::Layer(1))]), &steps).band, r, "from below");
        assert_eq!(touched(&changes(&[(r, Origin::Layer(2))]), &steps), Touch::NONE, "its own");
        assert_eq!(touched(&changes(&[(r, Origin::Layer(3))]), &steps), Touch::NONE, "above it");
        assert_eq!(touched(&changes(&[(r, Origin::Above)]), &steps), Touch::NONE, "the pointer");
        let far = Area::rect(1500, 900, 20, 10);
        assert_eq!(touched(&changes(&[(far, Origin::Zero)]), &steps), Touch::NONE, "out of reach");
        // A slice of the glass's own window below it is the window's own change.
        let own = [steps[0], step(200, 200, 600, 400, |l| l.layer_id = 7)];
        assert_eq!(touched(&changes(&[(r, Origin::Layer(1))]), &own), Touch::NONE);
    }

    #[test]
    fn a_change_ends_at_a_cover_and_test_reject_a_cover_for_an_unknown_origin() {
        let cover = step(320, 320, 200, 100, |l| l.opaque = true);
        let steps = [step(0, 0, 1920, 1080, |_| {}), cover];
        let r = Area::rect(350, 350, 20, 10);
        assert_eq!(touched(&changes(&[(r, Origin::Layer(0))]), &steps), Touch::NONE, "covered");
        // Uncovered (a held glass over displaced layers, P19-e): the cover is not asked.
        let raw = changes(&[(r, Origin::Layer(0))]).uncovered(&GLASS, &steps);
        assert_eq!((raw.band, raw.below), (r, true));
        // The cover's own change is not behind itself; half a cover covers nothing.
        assert_eq!(touched(&changes(&[(r, Origin::Layer(1))]), &steps).band, r);
        let wide = Area::rect(310, 350, 40, 10);
        assert_eq!(touched(&changes(&[(wide, Origin::Layer(0))]), &steps).band, wide);
        // An unknown origin may lie above the cover: it always reaches.
        for (unknown, zero) in [(Origin::Zero, true), (Origin::Stray, false)] {
            let t = touched(&changes(&[(r, unknown)]), &steps);
            assert_eq!((t.band, t.base, t.stray), (r, zero, !zero), "{unknown:?}");
        }
        // A faded cover shows what is below it; a glass covers with its drawn back alone.
        let faded = [steps[0], step(320, 320, 200, 100, |l| (l.opaque, l.opacity) = (true, 200))];
        assert_eq!(touched(&changes(&[(r, Origin::Layer(0))]), &faded).band, r);
        let mut pane = step(320, 320, 200, 100, |l| l.backdrop_blur = 40);
        assert_eq!(touched(&changes(&[(r, Origin::Layer(0))]), &[steps[0], pane]).band, r);
        let at = (320, 320, 200, 100);
        pane.glass = Some(Glass { cache: CacheStep::Read(0), rows: 27, band: Area::None, at });
        assert_eq!(touched(&changes(&[(r, Origin::Layer(0))]), &[steps[0], pane]), Touch::NONE);
    }

    #[test]
    fn a_written_glass_below_is_a_change_and_a_lost_change_blurs_everything() {
        let steps = [step(0, 0, 1920, 1080, |_| {}), step(100, 300, 180, 100, |l| l.layer_id = 3)];
        let mut c = changes(&[]);
        c.written(1, Area::rect(100, 300, 180, 100));
        let t = touched(&c, &steps);
        assert_eq!((t.band, t.glass, t.base), (Area::rect(234, 300, 46, 100), true, false));
        // Past its room the list says so, and every glass blurs whole — uncovered too.
        let mut c = Changes::new();
        assert!(c.begin(0));
        for _ in 0..=c.list.capacity() {
            c.written(1, Area::rect(0, 0, 1, 1));
        }
        assert_eq!(touched(&c, &steps), Touch::FULL);
        assert_eq!(c.uncovered(&GLASS, &steps), Touch::FULL);
    }

    #[test]
    fn layers_that_swapped_their_order_change_where_they_overlap() {
        let extent = |l: &FrameLayer| Area::rect(l.dst_x, l.dst_y, l.width, l.height);
        let drawn = [
            step(0, 0, 400, 300, |_| {}).layer,
            step(300, 200, 400, 300, |_| {}).layer,
            step(900, 900, 50, 50, |_| {}).layer,
        ];
        let mut c = changes(&[]);
        assert_eq!(c.reorder(&[Some(0), None, Some(1)], &drawn, extent), Area::None, "kept");
        assert_eq!(c.reorder(&[None, None, None], &drawn, extent), Area::None, "all new");
        let both = Area::rect(300, 200, 100, 100);
        assert_eq!(c.reorder(&[Some(1), Some(0), Some(2)], &drawn, extent), both);
        assert_eq!(c.list.len(), 1);
        assert_eq!(c.list[0].from, Origin::Order(0));
        // Swapped, but apart: nothing on the screen changed.
        assert_eq!(c.reorder(&[Some(0), Some(2), Some(1)], &drawn, extent), Area::None);
        assert_eq!(c.list.len(), 1);
    }

    #[test]
    fn a_desktop_glass_s_own_region_stays_its_own_content_uncovered() {
        // The id-0 exceptions of R1 hold in the uncovered mode: a change of the desktop inside
        // its own glass region is the region's content, one beside it reaches.
        let desk = GlassAt { id: 0, ..GLASS };
        let steps = [step(0, 0, 1920, 1080, |_| {}), step(0, 0, 1920, 40, |l| l.opaque = true)];
        let inside = Area::rect(350, 350, 20, 10);
        let beside = Area::rect(250, 350, 20, 10);
        for from in [Origin::Zero, Origin::Layer(0)] {
            assert_eq!(changes(&[(inside, from)]).uncovered(&desk, &steps), Touch::NONE);
            assert_eq!(changes(&[(beside, from)]).uncovered(&desk, &steps).band, beside);
        }
    }
}
