// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The compositor's damage-rect model — pure, host-TESTED, consumed by the OS-only
//! compositor runtime: the rect, its union, a rect's ORIGIN (the key of the layer that draws the
//! change, RFC-0101 §8 — the planner blurs a glass again only for a change below it), and the
//! bounded pending list a present collects its damage into. Rects of one origin join; rects of
//! two never do below the bound, and above it the join is of unknown origin and counted
//! (`windowd: loop … damage(rects= queued_px= flushed_px= degraded=)`). A rect of a megapixel
//! or more is named with its cause (`windowd: damage wide (…)`, an instrument, no marker).
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: unit tests below (the union, same-origin joins, two origins never join below
//!   the bound, nothing is dropped above it, the pre-merge, the wide line's cadence)
//! INVARIANTS: a queued rect is never lost — a dropped rect is a stale pixel on the screen; the
//!   pending list never exceeds `PENDING_DAMAGE_RECTS`; an origin is never above the layer that
//!   draws the change (unknown, `0`, is below everything: it costs a blur, never a stale one).

use alloc::string::String;
use core::sync::atomic::{AtomicU32, Ordering};

const DAMAGE_MERGE_AREA_PERCENT: u64 = 125;
const DAMAGE_MERGE_NEAR_GAP: u32 = 64;

/// The most damage rects a present keeps apart before they join.
pub(crate) const PENDING_DAMAGE_RECTS: usize = 6;
/// The most rects a present names: the pending ones, one re-blitted region, the pointer's.
pub(crate) const PRESENT_DAMAGE_RECTS: usize = PENDING_DAMAGE_RECTS + 2;
// The planner keeps `MAX_DAMAGE` rects of a present apart; one more and the frame is the screen.
const _: () = assert!(PRESENT_DAMAGE_RECTS <= nexus_gfx::frame::layers::MAX_DAMAGE);

/// A rect of this many pixels or more is named with its cause.
const WIDE_PX: u64 = 1_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DamageRect {
    pub(crate) x: u32,
    pub(crate) y: u32,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

impl DamageRect {
    const EMPTY: Self = Self { x: 0, y: 0, width: 0, height: 0 };

    pub(crate) const fn end_x(self) -> u32 {
        self.x.saturating_add(self.width)
    }

    pub(crate) const fn end_y(self) -> u32 {
        self.y.saturating_add(self.height)
    }

    pub(crate) fn merge(self, other: Self) -> Self {
        let x = self.x.min(other.x);
        let y = self.y.min(other.y);
        let end_x = self.end_x().max(other.end_x());
        let end_y = self.end_y().max(other.end_y());
        Self { x, y, width: end_x.saturating_sub(x), height: end_y.saturating_sub(y) }
    }

    /// The rect's pixels.
    pub(crate) fn area(self) -> u64 {
        u64::from(self.width).saturating_mul(u64::from(self.height))
    }

    const fn touches(self, other: Self) -> bool {
        other.x <= self.end_x()
            && other.end_x() >= self.x
            && other.y <= self.end_y()
            && other.end_y() >= self.y
    }
}

/// Why a rect was queued — the call sites' classes, for the wide line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Cause {
    /// A present's rect: base rows, or a motion layer's rows in the store.
    PresentRect,
    /// A present naming nothing, or a structure that changes the whole surface.
    PresentWhole,
    /// A region that moved, came or went (a motion layer, a scroll layer's viewport alike).
    MovedRegion,
    /// A scroll layer's band: its valid range, or its rows in the store.
    Band,
    /// A window shown — where the CPU composes (the GPU path names no rect for it).
    Shown,
    /// A window hidden or closed — where the CPU composes.
    Hidden,
    /// The caret came, went or moved.
    Caret,
    /// gpud refused a present: the screen again.
    Retry,
    /// A capture thawed: the screen again.
    Capture,
    /// The shell's base repainted (the wallpaper swapped, the reveal put off).
    Shell,
    /// A starting or closing surface came, went or gave way to the app's frame.
    Solid,
}

impl Cause {
    /// The causes: the wide line counts each on its own.
    const COUNT: usize = Self::Solid as usize + 1;

    const fn name(self) -> &'static str {
        match self {
            Self::PresentRect => "present-rect",
            Self::PresentWhole => "present-whole",
            Self::MovedRegion => "moved-region",
            Self::Band => "band",
            Self::Shown => "shown",
            Self::Hidden => "hidden",
            Self::Caret => "caret",
            Self::Retry => "retry",
            Self::Capture => "capture",
            Self::Shell => "shell",
            Self::Solid => "solid",
        }
    }
}

/// Where a rect's change is drawn: the key of the LOWEST layer that draws it (`FrameLayer.key`;
/// `0`: unknown — below everything), and why it was queued.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Origin {
    pub(crate) key: u32,
    pub(crate) cause: Cause,
}

impl Origin {
    /// The layer of `key` draws the change.
    pub(crate) const fn of(key: u32, cause: Cause) -> Self {
        Self { key, cause }
    }

    /// No layer is known to draw the change: it is below everything.
    pub(crate) const fn unknown(cause: Cause) -> Self {
        Self { key: 0, cause }
    }
}

/// The wide line of a queued rect of a megapixel or more, at its cause's 1st, 2nd, 4th, … time
/// with the count: it names where a screen-sized rect comes from without a line a frame.
pub(crate) fn wide_line(rect: DamageRect, origin: Origin) -> Option<String> {
    static SEEN: [AtomicU32; Cause::COUNT] = [const { AtomicU32::new(0) }; Cause::COUNT];
    if rect.area() < WIDE_PX {
        return None;
    }
    let n = SEEN.get(origin.cause as usize)?.fetch_add(1, Ordering::Relaxed).wrapping_add(1);
    n.is_power_of_two().then(|| {
        alloc::format!(
            "windowd: damage wide (px={} key={:#x} cause={} n={n})",
            rect.area(),
            origin.key,
            origin.cause.name()
        )
    })
}

#[allow(clippy::implicit_saturating_sub)]
fn damage_gap(a_start: u32, a_end: u32, b_start: u32, b_end: u32) -> u32 {
    if a_end < b_start {
        b_start - a_end
    } else if b_end < a_start {
        a_start - b_end
    } else {
        0
    }
}

fn should_premerge_damage(a: DamageRect, b: DamageRect) -> bool {
    let x_gap = damage_gap(a.x, a.end_x(), b.x, b.end_x());
    let y_gap = damage_gap(a.y, a.end_y(), b.y, b.end_y());
    let spatially_close = (x_gap == 0 && y_gap <= DAMAGE_MERGE_NEAR_GAP)
        || (y_gap == 0 && x_gap <= DAMAGE_MERGE_NEAR_GAP);
    if !spatially_close {
        return false;
    }
    let merged_area = a.merge(b).area();
    let source_area = a.area().saturating_add(b.area());
    merged_area.saturating_mul(100) <= source_area.saturating_mul(DAMAGE_MERGE_AREA_PERCENT).max(1)
}

/// Join near rects of ONE origin whose union grows little; returns the count that remain.
fn premerge(rects: &mut [(DamageRect, Origin)], mut count: usize) -> usize {
    let mut idx = 0;
    while idx < count {
        let mut scan = idx + 1;
        while scan < count {
            let (a, b) = (rects[idx], rects[scan]);
            if a.1.key == b.1.key && should_premerge_damage(a.0, b.0) {
                rects[idx].0 = a.0.merge(b.0);
                rects[scan] = rects[count - 1];
                count -= 1;
            } else {
                scan += 1;
            }
        }
        idx += 1;
    }
    count
}

/// The pixels the union of `a` and `b` holds beyond their own.
fn growth(a: DamageRect, b: DamageRect) -> u64 {
    a.merge(b).area().saturating_sub(a.area().saturating_add(b.area()))
}

/// The damage a present collects: at most [`PENDING_DAMAGE_RECTS`] rects, each with its origin.
pub(crate) struct PendingDamage {
    held: [(DamageRect, Origin); PENDING_DAMAGE_RECTS],
    len: usize,
}

impl PendingDamage {
    const NONE: (DamageRect, Origin) = (DamageRect::EMPTY, Origin::unknown(Cause::PresentRect));

    pub(crate) const fn new() -> Self {
        Self { held: [Self::NONE; PENDING_DAMAGE_RECTS], len: 0 }
    }

    pub(crate) const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Queue a rect: it joins the first rect of ITS origin it touches; touching none it takes a
    /// free place, or — the list full — joins the rect of its origin whose union grows the
    /// least. Only when the list is full and holds none of its origin are two of all the rects
    /// (the new one included) joined, the pair whose union grows the least: of two origins the
    /// join's is unknown (key `0`) and this returns `true` — the present's blur is no longer
    /// bounded by what changed. A rect is never dropped.
    #[must_use]
    pub(crate) fn queue(&mut self, rect: DamageRect, origin: Origin) -> bool {
        let same = |held: &(DamageRect, Origin)| held.1.key == origin.key;
        let live = &mut self.held[..self.len];
        if let Some(held) = live.iter_mut().find(|h| same(h) && h.0.touches(rect)) {
            held.0 = held.0.merge(rect);
            return false;
        }
        if let Some(free) = self.held.get_mut(self.len) {
            *free = (rect, origin);
            self.len += 1;
            return false;
        }
        let grows = |h: &&mut (DamageRect, Origin)| h.0.merge(rect).area() - h.0.area();
        if let Some(held) = self.held.iter_mut().filter(|h| same(h)).min_by_key(grows) {
            held.0 = held.0.merge(rect);
            return false;
        }
        // Over the bound: seven rects for six places.
        let mut all = [(rect, origin); PENDING_DAMAGE_RECTS + 1];
        all[..PENDING_DAMAGE_RECTS].copy_from_slice(&self.held);
        let (mut pair, mut least) = ((0, 1), u64::MAX);
        for i in 0..all.len() {
            for j in i + 1..all.len() {
                let g = growth(all[i].0, all[j].0);
                if g < least {
                    (pair, least) = ((i, j), g);
                }
            }
        }
        let (a, b) = (all[pair.0], all[pair.1]);
        let degraded = a.1.key != b.1.key;
        let joined = if degraded { Origin::unknown(a.1.cause) } else { a.1 };
        all[pair.0] = (a.0.merge(b.0), joined);
        all[pair.1] = all[PENDING_DAMAGE_RECTS];
        self.held.copy_from_slice(&all[..PENDING_DAMAGE_RECTS]);
        degraded
    }

    /// Everything queued, near rects of one origin joined, and the list empty: the rects and
    /// how many of them count.
    pub(crate) fn take(&mut self) -> ([(DamageRect, Origin); PENDING_DAMAGE_RECTS], usize) {
        let mut out = self.held;
        let count = premerge(&mut out, self.len);
        self.len = 0;
        (out, count)
    }

    #[cfg(test)]
    fn held(&self) -> &[(DamageRect, Origin)] {
        &self.held[..self.len]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn at(x: u32, y: u32) -> DamageRect {
        DamageRect { x, y, width: 10, height: 10 }
    }

    const fn key(key: u32) -> Origin {
        Origin::of(key, Cause::PresentRect)
    }

    /// Whether `inner` lies within `outer`.
    fn within(inner: DamageRect, outer: DamageRect) -> bool {
        outer.merge(inner) == outer
    }

    #[test]
    fn damage_rect_merge_bounds_small_target_updates() {
        let left = DamageRect { x: 10, y: 20, width: 30, height: 40 };
        let right = DamageRect { x: 32, y: 10, width: 20, height: 12 };
        assert_eq!(left.merge(right), DamageRect { x: 10, y: 10, width: 42, height: 50 });
    }

    #[test]
    fn damage_premerge_merges_only_bounded_area_growth() {
        let mut rects = [
            (DamageRect { x: 10, y: 10, width: 20, height: 20 }, key(7)),
            (DamageRect { x: 25, y: 10, width: 20, height: 20 }, key(7)),
            (DamageRect { x: 400, y: 400, width: 20, height: 20 }, key(7)),
        ];
        let count = premerge(&mut rects, 3);
        assert_eq!(count, 2);
        assert!(rects[..count].iter().any(|(r, _)| r.width == 35 && r.height == 20));
        assert!(rects[..count].iter().any(|(r, _)| r.x == 400 && r.y == 400));
    }

    #[test]
    fn test_reject_premerging_two_origins() {
        let mut rects = [
            (DamageRect { x: 10, y: 10, width: 20, height: 20 }, key(7)),
            (DamageRect { x: 25, y: 10, width: 20, height: 20 }, key(8)),
        ];
        assert_eq!(premerge(&mut rects, 2), 2);
        assert_eq!(rects[0].0.width, 20);
    }

    #[test]
    fn a_touching_rect_joins_its_neighbour_of_the_same_origin_in_place() {
        let mut pending = PendingDamage::new();
        assert!(pending.is_empty());
        assert!(!pending.queue(at(0, 0), key(7)));
        assert!(!pending.queue(at(5, 5), key(7)));
        assert_eq!(pending.held(), [(DamageRect { x: 0, y: 0, width: 15, height: 15 }, key(7))]);
        assert!(!pending.is_empty());
    }

    #[test]
    fn rects_of_one_origin_far_apart_stay_apart_below_the_bound() {
        // A bar pill and the dock, both the desktop's body: no 1920x1078 box between them.
        let mut pending = PendingDamage::new();
        assert!(!pending.queue(at(900, 0), key(7)));
        assert!(!pending.queue(at(900, 1000), key(7)));
        assert_eq!(pending.held(), [(at(900, 0), key(7)), (at(900, 1000), key(7))]);
    }

    #[test]
    fn test_reject_joining_two_origins_below_the_bound() {
        let mut pending = PendingDamage::new();
        assert!(!pending.queue(at(0, 0), key(7)));
        // Overlapping the first, of another layer: its own entry, the first untouched.
        assert!(!pending.queue(at(5, 5), key(8)));
        assert!(!pending.queue(at(5, 5), Origin::unknown(Cause::Shown)));
        assert_eq!(
            pending.held(),
            [(at(0, 0), key(7)), (at(5, 5), key(8)), (at(5, 5), Origin::unknown(Cause::Shown))]
        );
    }

    #[test]
    fn a_full_list_joins_the_rect_to_its_own_origin_and_never_another() {
        let mut pending = PendingDamage::new();
        for i in 0..PENDING_DAMAGE_RECTS as u32 {
            assert!(!pending.queue(at(100 * i, 0), key(1 + i % 2)));
        }
        // Nearest is x=500 (key 2); the nearest of ITS origin is x=400 (key 1).
        assert!(!pending.queue(at(530, 0), key(1)));
        assert_eq!(pending.held().len(), PENDING_DAMAGE_RECTS);
        let holder = pending.held().iter().find(|(r, _)| r.x == 400).expect("its origin's rect");
        assert_eq!((holder.0.end_x(), holder.1), (540, key(1)));
        assert!(pending.held().iter().filter(|(r, _)| r.x != 400).all(|(r, _)| r.width == 10));
    }

    #[test]
    fn test_reject_dropping_a_rect_above_the_bound() {
        let mut pending = PendingDamage::new();
        let rects: [DamageRect; PENDING_DAMAGE_RECTS + 1] =
            core::array::from_fn(|i| at(100 * i as u32, 37 * i as u32));
        let mut degraded = 0;
        for (i, r) in rects.iter().enumerate() {
            degraded += u32::from(pending.queue(*r, key(1 + i as u32)));
        }
        // Seven rects of seven layers in six places: one join, of unknown origin, counted.
        assert_eq!(degraded, 1);
        assert_eq!(pending.held().len(), PENDING_DAMAGE_RECTS);
        assert_eq!(pending.held().iter().filter(|(_, o)| o.key == 0).count(), 1);
        // Nothing is lost: every rect lies within an entry, and an entry that kept its origin
        // holds only its own rect.
        for (i, r) in rects.iter().enumerate() {
            assert!(pending.held().iter().any(|(h, _)| within(*r, *h)), "rect {i} was dropped");
            if let Some((h, _)) = pending.held().iter().find(|(_, o)| o.key == 1 + i as u32) {
                assert_eq!(h, r);
            }
        }
        let held: u64 = pending.held().iter().map(|(r, _)| r.area()).sum();
        assert!(held >= rects.iter().map(|r| r.area()).sum());
        // The flush takes them all and leaves the list empty.
        let (taken, count) = pending.take();
        assert_eq!(count, PENDING_DAMAGE_RECTS);
        assert!(rects.iter().all(|r| taken[..count].iter().any(|(h, _)| within(*r, *h))));
        assert!(pending.is_empty());
    }

    #[test]
    fn two_rects_of_one_origin_joined_above_the_bound_keep_it() {
        let mut pending = PendingDamage::new();
        // Two of key 9 far from each other, four others between them and far from all.
        assert!(!pending.queue(at(0, 0), key(9)));
        assert!(!pending.queue(at(40, 0), key(9)));
        for i in 0..4 {
            assert!(!pending.queue(at(1000 + 400 * i, 900), key(1 + i)));
        }
        // A seventh layer: the cheapest pair is the two of key 9 — no origin is lost.
        assert!(!pending.queue(at(3000, 3000), key(5)));
        assert!(pending.held().iter().all(|(_, o)| o.key != 0));
        assert!(pending.held().iter().any(|(r, o)| o.key == 9 && r.width == 50));
        assert!(pending.held().iter().any(|(r, o)| o.key == 5 && *r == at(3000, 3000)));
    }

    #[test]
    fn the_wide_line_names_a_megapixel_rect_at_its_first_second_and_fourth_time() {
        let screen = DamageRect { x: 0, y: 0, width: 1920, height: 1080 };
        // `Capture` is this test's own cause: the counts are per cause.
        let origin = Origin::unknown(Cause::Capture);
        assert_eq!(wide_line(at(0, 0), origin), None, "a small rect is never named");
        let said: [bool; 5] = core::array::from_fn(|_| wide_line(screen, origin).is_some());
        assert_eq!(said, [true, true, false, true, false]);
        assert_eq!(
            wide_line(screen, Origin::of(0x1_0100, Cause::PresentWhole)).as_deref(),
            Some("windowd: damage wide (px=2073600 key=0x10100 cause=present-whole n=1)")
        );
    }

    #[test]
    fn every_cause_has_its_own_name_and_a_present_names_what_the_planner_keeps() {
        let causes = [
            Cause::PresentRect,
            Cause::PresentWhole,
            Cause::MovedRegion,
            Cause::Band,
            Cause::Shown,
            Cause::Hidden,
            Cause::Caret,
            Cause::Retry,
            Cause::Capture,
            Cause::Shell,
            Cause::Solid,
        ];
        assert_eq!(causes.len(), Cause::COUNT);
        for (i, c) in causes.iter().enumerate() {
            assert_eq!(*c as usize, i, "the wide line counts {} in its own place", c.name());
            assert!(causes[..i].iter().all(|other| other.name() != c.name()));
        }
        assert_eq!(PRESENT_DAMAGE_RECTS, nexus_gfx::frame::layers::MAX_DAMAGE);
    }
}
