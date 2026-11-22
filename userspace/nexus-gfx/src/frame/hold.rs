// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The glass hold (TASK-0351 P6, RFC-0109): what moves is not blurred again until it
//! stops. A glass's cache rows hold the blur of its layer's REST rectangle — the layer as the
//! present encoded it, its window's transform not applied — so the rows a panel was given on
//! its first frame are the rows it rests with, whatever size it is drawn at in between. One
//! rule keeps them: a glass with rows is HELD — read, never written — while its own window is
//! in motion, or while a layer in motion lies below it with its drawn extent, now or in the
//! last plan, touching its reach. The hold is bounded twice: a glass is held at most
//! [`HOLD_MAX`] plans in a row (a client that parks a transform starves no glass of the
//! changes below it), and a glass at rest over a mover at most until the mover's edges
//! travelled its own blur radius (a dragged window re-blurs the dock every radius of travel,
//! not every frame and not never). The plan that ends a hold takes the walk's usual path, so
//! the glass pays there what it owes.
//! OWNERS: @gpu @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: unit tests below; `tests/frame_motion.rs` (the write counts of a scale-in, a
//!   fade, a drag, a parked transform), `tests/frame_reach_oracle.rs` (rows at rest against a
//!   full redraw)
//! INVARIANTS: the hold decides read or not and nothing else — what a held plan owes is the
//!   walk's (`Frame::plan`); the tables are the plan's own storage, reserved once a plan; a
//!   mover or a run that found no room holds nothing (the glass is written by its key).

use alloc::vec::Vec;

use super::damage::Area;
use super::glass::FullBy;
use super::layers::FrameLayer;
use super::reach::{GlassAt, GLASS_REACH};

/// The hold's decisions over a plan's glasses (`Frame::plan`, `gpud: glass holds (`), counted
/// by the reason that decided — counters beside the rule, never an input to it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Holds {
    /// Held: read from the rows its blur was kept in.
    pub held: u16,
    /// Not held: nothing in motion — its own window rests and no mover lies within its reach.
    pub rest: u16,
    /// Not held: the movers below it travelled past its blur radius.
    pub drift: u16,
    /// Not held: held `HOLD_MAX` plans in a row.
    pub bound: u16,
    /// Not held: no rows to keep — a full present, no rows yet, or rows of another rectangle.
    pub full: u16,
    /// Of the held: made to owe a change made while the layers below it are displaced — its
    /// area within the reach, uncovered (`Changes::uncovered`, P19-e).
    pub foreign: u16,
}

impl Holds {
    /// No glass decided yet.
    pub const NONE: Self = Self { held: 0, rest: 0, drift: 0, bound: 0, full: 0, foreign: 0 };

    /// `other`'s decisions added (a window of plans).
    pub fn add(&mut self, other: Self) {
        self.held = self.held.saturating_add(other.held);
        self.rest = self.rest.saturating_add(other.rest);
        self.drift = self.drift.saturating_add(other.drift);
        self.bound = self.bound.saturating_add(other.bound);
        self.full = self.full.saturating_add(other.full);
        self.foreign = self.foreign.saturating_add(other.foreign);
    }
}

/// What a glass held while the layers below it are displaced (a key miss) owes after this plan
/// (TASK-0351 P19-e), and the rule named should that be the whole glass. `owed`: what it owed;
/// `raw`: this plan's changes within its reach, no cover asked (`Changes::uncovered`) — a layer
/// that does not move rests where its change is named, only the covers above it may move;
/// `moved`: what the layers below changed since the rows' key (the displacement so far and this
/// plan's `Moves::below`; `None`: nothing followed them). A layer that moves rests elsewhere:
/// at a key miss the write's band holds the displacement (C-r), but back where the rows were
/// blurred (a key hit) only the owe is paid — so once any change is owed, so is `moved`. Whole
/// for a lost change list (`raw` whole) or a displacement not followed.
pub(super) fn displaced_owe(owed: Area, raw: Area, moved: Option<Area>) -> (Area, FullBy) {
    if raw == Area::None {
        return (owed, FullBy::Lost);
    }
    let by = if raw == Area::Full { FullBy::Lost } else { FullBy::Displaced };
    (owed.union(raw).union(moved.unwrap_or(Area::Full)), by)
}

/// The plans a glass is held in a row before it pays what it owes.
pub(super) const HOLD_MAX: u8 = 64;

// The longest transition windowd runs is 31 frames: a hold spans it twice over, so the bound
// costs a transition no write.
const _: () = assert!(HOLD_MAX > 2 * 31);

/// A layer of a window in motion, as the walk passed it.
#[derive(Clone, Copy)]
struct Mover {
    /// Its window's transform id.
    id: u32,
    /// The screen it covers now and covered in the last plan.
    span: Area,
    /// How far its edges moved since the last plan (the farthest of the four).
    shift: u32,
}

/// A glass's hold so far: the plans it was held in a row, how far the movers below it
/// travelled meanwhile, and the rest rectangle and blur radius it had in the last plan — rows
/// are of those, so a glass the set moved or resized is not held (it is written by its key).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Run {
    plans: u8,
    drift: u32,
    rect: Area,
    radius: u32,
}

/// The holds of a plan: the movers of the walk so far, and a run per layer, carried from plan
/// to plan by the layer's identity as the cache's slots are.
pub(super) struct Hold {
    movers: Vec<Mover>,
    runs: Vec<Run>,
    /// The last plan's runs while `carry` hands them on.
    last: Vec<Run>,
    /// What [`Self::holds`] decided this plan, counted (TASK-0373: counters, never a rule).
    counts: Holds,
}

/// The farthest any edge of `now`'s rectangle lies from where `before` had it.
fn shift(now: &FrameLayer, before: &FrameLayer) -> u32 {
    let edges = |l: &FrameLayer| {
        [l.dst_x, l.dst_y, l.dst_x.saturating_add(l.width), l.dst_y.saturating_add(l.height)]
    };
    edges(now).iter().zip(edges(before)).map(|(a, b)| a.abs_diff(b)).max().unwrap_or(0)
}

impl Hold {
    pub(super) const fn new() -> Self {
        Self { movers: Vec::new(), runs: Vec::new(), last: Vec::new(), counts: Holds::NONE }
    }

    /// Emptied for a plan of `layers` layers, the last plan's runs kept for [`Self::carry`];
    /// `false` when the heap refuses the room.
    pub(super) fn begin(&mut self, layers: usize) -> bool {
        core::mem::swap(&mut self.runs, &mut self.last);
        self.movers.clear();
        self.runs.clear();
        self.counts = Holds::NONE;
        self.movers.try_reserve(layers).is_ok() && self.runs.try_reserve(layers).is_ok()
    }

    /// The runs follow their layers into the plan: layer `i` takes the run of the last plan's
    /// step `carried[i]`, a new layer none.
    pub(super) fn carry(&mut self, carried: &[Option<usize>]) {
        let (room, last) = (self.runs.capacity().min(carried.len()), &self.last);
        self.runs.extend(
            carried[..room]
                .iter()
                .map(|from| from.and_then(|j| last.get(j)).copied().unwrap_or_default()),
        );
    }

    /// A layer of a window in motion, drawn as `now` and in the last plan as `before` (`None`:
    /// it is new); `extent`: the screen a layer covers.
    pub(super) fn mover(
        &mut self,
        now: &FrameLayer,
        before: Option<&FrameLayer>,
        extent: fn(&FrameLayer) -> Area,
    ) {
        if self.movers.len() == self.movers.capacity() {
            return;
        }
        let span = before.map_or(Area::None, extent).union(extent(now));
        let shift = before.map_or(0, |b| shift(now, b));
        self.movers.push(Mover { id: now.layer_id, span, shift });
    }

    /// Whether glass `g` is held this plan. `rows`: it has rows to keep and the present is not
    /// a full one; `own`: its own window is in motion; `radius`: its blur's. The movers noted
    /// so far are the ones below it. A plan that does not hold ends the run: the next hold
    /// starts again.
    pub(super) fn holds(&mut self, g: &GlassAt, rows: bool, own: bool, radius: u32) -> bool {
        let reach = g.rect.grown(GLASS_REACH);
        let below = self.movers.iter().filter(|m| m.id != g.id && m.span.touches(reach));
        let over = below.map(|m| m.shift).max();
        let Some(run) = self.runs.get_mut(g.index) else { return false };
        // The drift bounds a glass at rest over a mover; its own motion moves the glass with
        // what it shows.
        let drift = match (own, over) {
            (true, _) => Some(0),
            (false, Some(shift)) => Some(run.drift.saturating_add(shift)),
            (false, None) => None,
        };
        let same = (run.rect, run.radius) == (g.rect, radius);
        let held = drift.filter(|d| rows && same && run.plans < HOLD_MAX && *d <= radius);
        // Why, counted beside the rule (the first reason that refuses, in the rule's order).
        let c = &mut self.counts;
        let n = match drift {
            _ if held.is_some() => &mut c.held,
            None => &mut c.rest,
            Some(_) if !(rows && same) => &mut c.full,
            Some(_) if run.plans >= HOLD_MAX => &mut c.bound,
            Some(_) => &mut c.drift,
        };
        *n = n.saturating_add(1);
        let plans = held.map_or(0, |_| run.plans + 1);
        *run = Run { plans, drift: held.unwrap_or(0), rect: g.rect, radius };
        held.is_some()
    }

    /// A glass held this plan owes a change made while the layers below are displaced
    /// (`Changes::uncovered`) — counted (TASK-0351 P6, `Holds::foreign`), no rule.
    pub(super) fn foreign(&mut self) {
        self.counts.foreign = self.counts.foreign.saturating_add(1);
    }

    /// What [`Self::holds`] decided in this plan.
    pub(super) const fn counts(&self) -> Holds {
        self.counts
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layer(id: u32, x: u32, y: u32, w: u32, h: u32) -> FrameLayer {
        FrameLayer {
            layer_id: id,
            dst_x: x,
            dst_y: y,
            width: w,
            height: h,
            opacity: 255,
            ..FrameLayer::EMPTY
        }
    }

    fn extent(l: &FrameLayer) -> Area {
        Area::rect(l.dst_x, l.dst_y, l.width, l.height)
    }

    const GLASS: GlassAt =
        GlassAt { index: 1, rect: Area::Rect { x0: 300, y0: 300, x1: 700, y1: 500 }, id: 7 };

    /// A plan of two layers, the glass the second, the mover `(now, before)` below it.
    fn plan(hold: &mut Hold, mover: Option<(FrameLayer, Option<FrameLayer>)>) {
        assert!(hold.begin(2));
        hold.carry(&[Some(0), Some(1)]);
        if let Some((now, before)) = mover {
            hold.mover(&now, before.as_ref(), extent);
        }
    }

    /// The holds after the plan that wrote the glass: its run knows its rectangle.
    fn written() -> Hold {
        let mut hold = Hold::new();
        plan(&mut hold, None);
        assert!(!hold.holds(&GLASS, false, true, 40), "no rows yet");
        hold
    }

    #[test]
    fn a_glass_is_held_for_its_own_motion_or_a_mover_within_its_reach() {
        let mut hold = written();
        plan(&mut hold, None);
        assert!(!hold.holds(&GLASS, true, false, 40), "nothing moves");
        assert!(hold.holds(&GLASS, true, true, 40), "its own window does");
        assert!(!hold.holds(&GLASS, false, true, 40), "no rows to keep");
        // A mover below within the reach holds it; one elsewhere, or of its own window, not.
        plan(&mut hold, Some((layer(3, 100, 300, 150, 100), None)));
        assert!(hold.holds(&GLASS, true, false, 40));
        plan(&mut hold, Some((layer(3, 100, 700, 150, 100), None)));
        assert!(!hold.holds(&GLASS, true, false, 40), "out of reach");
        plan(&mut hold, Some((layer(7, 100, 300, 150, 100), None)));
        assert!(!hold.holds(&GLASS, true, false, 40), "its own window's layer");
        // Where the mover was in the last plan counts: the frame it leaves the reach.
        let (now, before) = (layer(3, 100, 570, 150, 100), layer(3, 100, 540, 150, 100));
        plan(&mut hold, Some((now, Some(before))));
        assert!(hold.holds(&GLASS, true, false, 40), "{:?}", hold.runs[1]);
        // The set moved the glass, or gave it another blur: its rows are of another glass.
        let moved = GlassAt { rect: Area::rect(310, 300, 400, 200), ..GLASS };
        for (g, radius) in [(moved, 40), (moved, 20)] {
            plan(&mut hold, None);
            assert!(!hold.holds(&g, true, true, radius), "not as its rows were blurred");
            plan(&mut hold, None);
            assert!(hold.holds(&g, true, true, radius), "written: held again");
        }
    }

    #[test]
    fn test_reject_a_hold_past_its_plans_or_past_a_blur_radius_of_drift() {
        let mut hold = written();
        // A parked transform: held `HOLD_MAX` plans, then not, then again.
        for n in 0..=u32::from(HOLD_MAX) * 2 + 1 {
            plan(&mut hold, None);
            let pays = n % (u32::from(HOLD_MAX) + 1) == u32::from(HOLD_MAX);
            assert_eq!(hold.holds(&GLASS, true, true, 40), !pays, "plan {n}");
        }
        // A mover travelling 15 pixels a plan under a blur of 40: the third plan is past it,
        // and the drift starts again.
        let mut hold = written();
        for n in 0..9u32 {
            let at = |n: u32| layer(3, 100 + 15 * n, 300, 150, 100);
            plan(&mut hold, Some((at(n + 1), Some(at(n)))));
            assert_eq!(hold.holds(&GLASS, true, false, 40), n % 3 != 2, "plan {n}");
        }
        // Its own motion carries no drift: the glass moves with what it shows.
        for _ in 0..9 {
            let (now, before) = (layer(3, 190, 300, 150, 100), layer(3, 100, 300, 150, 100));
            plan(&mut hold, Some((now, Some(before))));
            assert!(hold.holds(&GLASS, true, true, 40));
        }
    }

    #[test]
    fn the_holds_are_counted_by_the_reason_that_decided() {
        let mut hold = written();
        assert_eq!(hold.counts(), Holds { full: 1, ..Holds::NONE }, "no rows yet");
        plan(&mut hold, None);
        assert!(!hold.holds(&GLASS, true, false, 40));
        assert!(hold.holds(&GLASS, true, true, 40));
        assert_eq!(hold.counts(), Holds { held: 1, rest: 1, ..Holds::NONE });
        let far = |x| layer(3, x, 300, 150, 100);
        plan(&mut hold, Some((far(190), Some(far(100)))));
        assert!(!hold.holds(&GLASS, true, false, 40));
        assert_eq!(hold.counts(), Holds { drift: 1, ..Holds::NONE }, "a plan starts at zero");
        for _ in 0..=HOLD_MAX {
            plan(&mut hold, None);
            let _ = hold.holds(&GLASS, true, true, 40);
        }
        assert_eq!(hold.counts(), Holds { bound: 1, ..Holds::NONE }, "held HOLD_MAX in a row");
    }

    #[test]
    fn test_reject_a_bounded_owe_after_a_lost_change_list() {
        let (owed, change) = (Area::rect(320, 320, 10, 10), Area::rect(400, 400, 20, 10));
        let moved = Some(Area::rect(250, 350, 80, 60));
        // A lost list (`Changes::uncovered` is whole for it) owes the whole glass.
        assert_eq!(displaced_owe(owed, Area::Full, moved), (Area::Full, FullBy::Lost));
        // A change owes its area and the displacement so far — whole when nothing followed it.
        let all = owed.union(change).union(Area::rect(250, 350, 80, 60));
        assert_eq!(displaced_owe(owed, change, moved).0, all);
        assert_eq!(displaced_owe(owed, change, None), (Area::Full, FullBy::Displaced));
        // No change: the owe stays what it was.
        assert_eq!(displaced_owe(owed, Area::None, None).0, owed);
    }

    #[test]
    fn a_run_follows_its_layer_and_a_layer_without_room_is_not_held() {
        let mut hold = written();
        plan(&mut hold, None);
        assert!(hold.holds(&GLASS, true, true, 40));
        // The glass is the first layer now: its run came with it.
        assert!(hold.begin(1));
        hold.carry(&[Some(1)]);
        assert_eq!(hold.runs[0], Run { plans: 1, drift: 0, rect: GLASS.rect, radius: 40 });
        assert!(!hold.holds(&GLASS, true, true, 40), "no run at its index");
    }
}
