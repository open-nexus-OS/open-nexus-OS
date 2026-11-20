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
//!    above the cover.
//! R3 a glass blurred again is a change of its own step for the glasses above it.
//! R4 layers that swapped their order changed the screen where they overlap, though neither is
//!    drawn otherwise.
//! OWNERS: @gpu @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! The writes are counted by what reached the glass (`WritesBy`: an unknown origin split into
//! `zero` and `stray`, the key misses at rest by what differs — counters, TASK-0351 P19).
//! TEST_COVERAGE: unit tests below; `tests/frame_reach.rs` (the rules' write counts),
//!   `tests/frame_reach_oracle.rs` (a planned frame against a full redraw, 200 presents),
//!   `tests/frame_rest_misses.rs` (the counters through a plan)
//! INVARIANTS: a change is kept from a glass only by a proof (its step is the glass or above it,
//!   or a cover holds it) — an unknown origin and an uncertain cover always reach; the list is
//!   the plan's own storage (reserved once a plan, never grown in the walk), and a change it
//!   could not hold makes every glass blur whole; a key is searched for, never an index.

use alloc::vec::Vec;

use super::damage::Area;
use super::glass::{CacheStep, Miss, WriteCause};
use super::layers::{FrameLayer, RetainedLayers, MAX_DAMAGE, ORIGIN_ABOVE};
use super::occlude::{self, Px};
use super::plan::LayerStep;

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

/// The glass writes of a frame by what reached the glass (TASK-0351 P6: `gpud: glass writes
/// by`) — beside [`super::plan::Writes`], which counts them by cause. A write has one class or
/// none: a full present, new rows and a put-off write paid later carry none.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WritesBy {
    /// A band: a named change of a layer below the glass.
    pub below: u16,
    /// A band: a glass below it was blurred again (R3), and nothing named reached it.
    pub glass: u16,
    /// A band: a change of origin 0 or of a key the set does not hold reached it — a call site
    /// that queues damage without its layer. The sum of `zero` and `stray`.
    pub unknown: u16,
    /// Of `unknown`: a change of origin 0 reached it (TASK-0351 P19, W4: windowd queued it
    /// without its layer — `windowd: damage zero (…)` names where).
    pub zero: u16,
    /// Of `unknown`: only a change of a key no layer of the set carries reached it (a culled or
    /// gone layer's key).
    pub stray: u16,
    /// The backdrop: a layer below the glass came, went or moved.
    pub moved: u16,
    /// Layers below the glass swapped their order (R4).
    pub order: u16,
    /// The one write after the glass kept its rows through a motion.
    pub rest: u16,
    /// Of the `rest` writes, those of a key miss, by what differs (TASK-0351 P19, W5).
    pub misses: RestMisses,
}

/// The writes that end a hold with a key miss, by the first of the glass's prints that differs
/// from the ones its rows were written with (TASK-0351 P19, W5: `gpud: glass rest misses`) —
/// counters, never an input to a rule.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RestMisses {
    /// Layers below within its reach came, went or changed their order (or the rows carry no
    /// prints).
    pub set: u16,
    /// Only layers of the glass's own window moved or resized.
    pub own: u16,
    /// Layers of other windows below it moved or resized.
    pub moved: u16,
    /// The same layers at the same places: something else the key folds changed (an opacity, a
    /// source, a corner radius, the glass's own rectangle or radii).
    pub drew: u16,
    /// The wallpaper the frames start from was uploaded again.
    pub epoch: u16,
    /// Of all of these, the ones written as a band (C-r: what changed below while the rows
    /// were kept, `Frame::plan`) that blurs less than the whole glass — a band that reaches all
    /// of it costs the whole glass and is not counted. Never more than their sum.
    pub band: u16,
}

impl RestMisses {
    fn note(&mut self, m: Miss) {
        let n = match m {
            Miss::Set => &mut self.set,
            Miss::Own => &mut self.own,
            Miss::Moved => &mut self.moved,
            Miss::Drew => &mut self.drew,
            Miss::Epoch => &mut self.epoch,
        };
        *n = n.saturating_add(1);
    }
}

impl WritesBy {
    /// Counts one write of `cause` for what `touch` says reached the glass; `rested`: the last
    /// plan kept the glass's rows through a motion; `differs`: what differs from the prints the
    /// rows were written with (`GlassCache::printed`), counted for a key miss at rest.
    pub(super) fn note(
        &mut self,
        touch: &Touch,
        cause: WriteCause,
        rested: bool,
        differs: Option<Miss>,
    ) {
        if let (true, WriteCause::Backdrop, Some(m)) = (rested, cause, differs) {
            self.misses.note(m);
        }
        let n = match cause {
            WriteCause::Full => return,
            _ if rested => &mut self.rest,
            WriteCause::Backdrop if touch.order => &mut self.order,
            WriteCause::Backdrop => &mut self.moved,
            WriteCause::Band if touch.base || touch.stray => {
                let split = if touch.base { &mut self.zero } else { &mut self.stray };
                *split = split.saturating_add(1);
                &mut self.unknown
            }
            WriteCause::Band if touch.below => &mut self.below,
            WriteCause::Band if touch.glass => &mut self.glass,
            WriteCause::Band if touch.order => &mut self.order,
            _ => return,
        };
        *n = n.saturating_add(1);
    }
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
        if self.lost {
            return Touch::FULL;
        }
        let reach = g.rect.grown(GLASS_REACH);
        let id_of = |i: usize| steps.get(i).map(|s| s.layer.layer_id);
        let mut t = Touch::NONE;
        for c in self.list.iter().filter(|c| c.area.touches(reach)) {
            let within = c.area.intersect(reach);
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
                        && !covered(within, i, steps, covers)
                }
                Origin::Glass(i) => {
                    i < g.index
                        && id_of(i).is_some_and(|id| id == 0 || id != g.id)
                        && !covered(within, i, steps, covers)
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

    /// Whether the frame changes anything that may be part of a backdrop of the glass of
    /// window `id` in ANOTHER scene than this frame's (`drawn`: the set as drawn) — asked for a
    /// glass held while what lies below it is displaced. A change is named where its layer is
    /// drawn now and ends at the covers that stand now: once the layers are back where the
    /// glass's rows were blurred, it may lie anywhere. Only the pointer's changes and those of
    /// the glass's own window are none of its backdrop wherever they lie.
    pub(super) fn foreign(&self, id: u32, drawn: &[FrameLayer]) -> bool {
        self.lost
            || self.list.iter().any(|c| match c.from {
                Origin::Above => false,
                Origin::Layer(i) | Origin::Glass(i) => {
                    id == 0 || drawn.get(i).is_none_or(|l| l.layer_id != id)
                }
                Origin::Zero | Origin::Stray | Origin::Order(_) => true,
            })
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
        // Past its room the list says so, and every glass blurs whole.
        let mut c = Changes::new();
        assert!(c.begin(0));
        for _ in 0..=c.list.capacity() {
            c.written(1, Area::rect(0, 0, 1, 1));
        }
        assert_eq!(touched(&c, &steps), Touch::FULL);
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
    fn a_write_is_counted_once_by_what_reached_it() {
        let mut by = WritesBy::default();
        let t = |f: fn(&mut Touch)| {
            let mut t = Touch::NONE;
            f(&mut t);
            t
        };
        by.note(&t(|t| (t.base, t.below) = (true, true)), WriteCause::Band, false, None);
        by.note(&t(|t| (t.below, t.glass) = (true, true)), WriteCause::Band, false, None);
        by.note(&t(|t| t.glass = true), WriteCause::Band, false, None);
        by.note(&t(|t| t.order = true), WriteCause::Backdrop, false, None);
        by.note(&t(|_| {}), WriteCause::Backdrop, false, None);
        by.note(&t(|t| t.below = true), WriteCause::Backdrop, true, None);
        by.note(&Touch::FULL, WriteCause::Full, true, None);
        by.note(&t(|_| {}), WriteCause::New, false, None);
        let all = WritesBy {
            below: 1,
            glass: 1,
            unknown: 1,
            zero: 1,
            moved: 1,
            order: 1,
            rest: 1,
            ..Default::default()
        };
        assert_eq!(by, all);
    }

    #[test]
    fn an_unknown_origin_is_counted_as_zero_or_stray_and_their_sum_is_unknown() {
        let mut by = WritesBy::default();
        let (zero, stray, both) = (
            Touch { base: true, ..Touch::NONE },
            Touch { stray: true, ..Touch::NONE },
            Touch { base: true, stray: true, below: true, ..Touch::NONE },
        );
        for t in [zero, stray, both, stray] {
            by.note(&t, WriteCause::Band, false, None);
        }
        assert_eq!((by.zero, by.stray, by.unknown, by.below), (2, 2, 4, 0));
        // A write at rest is `rest` whatever reached it: neither zero nor stray.
        by.note(&zero, WriteCause::Band, true, None);
        assert_eq!((by.zero + by.stray, by.unknown, by.rest), (4, 4, 1));
    }

    #[test]
    fn a_key_miss_at_rest_is_counted_by_what_differs_and_no_other_write_is() {
        let mut by = WritesBy::default();
        let none = Touch::NONE;
        for m in [Miss::Set, Miss::Own, Miss::Moved, Miss::Drew, Miss::Epoch, Miss::Set] {
            by.note(&none, WriteCause::Backdrop, true, Some(m));
        }
        // Not at rest, not a key miss, or nothing to compare: not a rest miss.
        by.note(&none, WriteCause::Backdrop, false, Some(Miss::Own));
        by.note(&none, WriteCause::Band, true, Some(Miss::Own));
        by.note(&none, WriteCause::New, true, Some(Miss::Own));
        by.note(&none, WriteCause::Full, true, Some(Miss::Own));
        by.note(&none, WriteCause::Backdrop, true, None);
        let misses = RestMisses { set: 2, own: 1, moved: 1, drew: 1, epoch: 1, band: 0 };
        assert_eq!((by.misses, by.rest), (misses, 9));
    }
}
