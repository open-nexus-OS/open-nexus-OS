// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the paged track's pure decisions for `.scroll(paged)` viewports (the launcher's
//! pages — design artifacts: PagedView, TASK-0354 P12): a drag follows 1:1 once the axis locked,
//! past either end it rubber-bands at 35 % (also with a single page), a release turns a page past
//! 16 % of the width or after a flick, the wheel turns past 40 units and then cools, and every
//! turn or return is a 500 ms glide on the glide curve. `probe/scroll/paged.rs` runs them per
//! frame; the decisions live here so host tests reach them (`probe/` is RISC-V-only). The
//! offsets these produce may leave `[0, max]` by at most one viewport: windowd intersects any
//! offset with the band's valid range (RFC-0109).
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `pager_math/tests.rs` (the spec's numbers, named)

/// The pointer travel (px) before a press in a paged viewport decides its axis.
pub(crate) const AXIS_LOCK_PX: i32 = 6;
/// The share of a drag past either end that the track follows (rubber band).
pub(crate) const RUBBER_PCT: i32 = 35;
/// A release turns a page once the drag passed this share of the viewport's width.
pub(crate) const TURN_WIDTH_PCT: i32 = 16;
/// A flick: a drag of more than this many px …
pub(crate) const FLICK_PX: i32 = 24;
/// … within this time turns a page whatever its share of the width.
pub(crate) const FLICK_NS: u64 = 250_000_000;
/// The wheel units past which an accumulated wheel turns a page.
pub(crate) const WHEEL_TURN: i32 = 40;
/// One wheel detent in the design's wheel units (a prototype convention: one detent, one page).
pub(crate) const WHEEL_UNITS_PER_NOTCH: i32 = 100;
/// After a wheel turn the wheel is ignored this long.
pub(crate) const WHEEL_COOLDOWN_NS: u64 = 420_000_000;
/// The glide's curve (the theme's `glide`; its time is the theme's `slow` step, 500 ms).
pub(crate) const GLIDE_CURVE: [f32; 4] = [0.22, 1.0, 0.36, 1.0];

/// What a press in a paged viewport became once it moved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Lock {
    /// Not far enough yet.
    Undecided,
    /// Along the track: the track follows the pointer, the press is no tap.
    Track,
    /// Across the track: an ordinary drag of what lies under the press.
    Cross,
}

/// The axis a press that moved `(dx, dy)` locks to: more than `AXIS_LOCK_PX` along the track and
/// more along than across → the track; more than `AXIS_LOCK_PX` across → across; else undecided.
pub(crate) fn axis_lock(dx: i32, dy: i32) -> Lock {
    let (ax, ay) = (dx.unsigned_abs(), dy.unsigned_abs());
    let lock = AXIS_LOCK_PX.unsigned_abs();
    if ax > lock && ax > ay {
        Lock::Track
    } else if ay > lock {
        Lock::Cross
    } else {
        Lock::Undecided
    }
}

/// The track's offset for a raw offset `raw` on a track of extent `[0, max_x]`: inside 1:1,
/// beyond either end the part past it at `RUBBER_PCT`, never farther than one viewport `view_w`
/// past the end (the bound windowd's intersection relies on; no overflow at the integer's ends).
pub(crate) fn rubber(raw: i32, max_x: i32, view_w: i32) -> i32 {
    let (raw, max, view) = (i64::from(raw), i64::from(max_x.max(0)), i64::from(view_w.max(0)));
    let past = |over: i64| over * i64::from(RUBBER_PCT) / 100;
    let out = if raw < 0 {
        past(raw).max(-view)
    } else if raw > max {
        (max + past(raw - max)).min(max + view)
    } else {
        raw
    };
    out.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

/// The offset while the pointer dragged `dx` px from a press on the page at `page_px` (a drag to
/// the left, `dx < 0`, moves toward the next page).
pub(crate) fn drag_offset(page_px: i32, dx: i32, max_x: i32, view_w: i32) -> i32 {
    rubber(page_px.saturating_sub(dx), max_x, view_w)
}

/// The page a drag of `dx` px over `dt_ns` from page `page` (of `0..=last`) releases to: one page
/// on (`dx < 0`: the next) past `TURN_WIDTH_PCT` of the width, or after a flick; never past the
/// ends.
pub(crate) fn release_page(page: i32, last: i32, dx: i32, dt_ns: u64, view_w: i32) -> i32 {
    let ax = i64::from(dx.unsigned_abs());
    let far = ax * 100 > i64::from(view_w.max(0)) * i64::from(TURN_WIDTH_PCT);
    let flick = ax > i64::from(FLICK_PX) && dt_ns < FLICK_NS;
    if dx == 0 || !(far || flick) {
        return page;
    }
    (page - dx.signum()).clamp(0, last.max(0))
}

/// The wheel units of `notches` detents (wheel down, a negative notch, is the next page — the
/// polarity of vertical scrolling).
pub(crate) fn notch_units(notches: i32) -> i32 {
    notches.saturating_neg().saturating_mul(WHEEL_UNITS_PER_NOTCH)
}

/// One wheel event of `units` (positive: toward the next page) on an accumulator `acc`: while
/// cooling the event is dropped; past `WHEEL_TURN` the accumulator empties and the page turns
/// (`Some(+1)` next, `Some(-1)` previous) — the caller starts the cooldown.
pub(crate) fn wheel_step(acc: i32, units: i32, cooling: bool) -> (i32, Option<i32>) {
    if cooling {
        return (acc, None);
    }
    let acc = acc.saturating_add(units);
    if acc.unsigned_abs() > WHEEL_TURN.unsigned_abs() {
        (0, Some(acc.signum()))
    } else {
        (acc, None)
    }
}

/// How a press in a paged viewport ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GestureEnd {
    /// It never locked to the track and held a tap: the tap fires now, at the release.
    Tap,
    /// It dragged the track: the track releases (`release_page`), no tap.
    Track,
    /// Neither (an ordinary drag across, or nothing to tap).
    Nothing,
}

/// The end of a press that locked as `lock` and holds a tap (`held_tap`): exactly one of a tap
/// and a track drag, never both.
pub(crate) fn gesture_end(lock: Lock, held_tap: bool) -> GestureEnd {
    match (lock, held_tap) {
        (Lock::Track, _) => GestureEnd::Track,
        (Lock::Undecided, true) => GestureEnd::Tap,
        _ => GestureEnd::Nothing,
    }
}

/// A turn or a return on its way: from `from_px` to `to_px`, started at `start_ns`, over
/// `dur_ns` (the theme's `slow` step; 0 under reduced motion — it lands at once).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PageGlide {
    pub(crate) from_px: i32,
    pub(crate) to_px: i32,
    pub(crate) start_ns: u64,
    pub(crate) dur_ns: u64,
}

impl PageGlide {
    /// The offset at `now_ns` and whether the glide is done.
    pub(crate) fn at(&self, now_ns: u64) -> (i32, bool) {
        if self.dur_ns == 0 || now_ns >= self.start_ns.saturating_add(self.dur_ns) {
            return (self.to_px, true);
        }
        let t = now_ns.saturating_sub(self.start_ns) as f32 / self.dur_ns as f32;
        let e = animation::keyframe::cubic_bezier(GLIDE_CURVE, t);
        let span = (i64::from(self.to_px) - i64::from(self.from_px)) as f32;
        // Half away from zero (`f32::round` is `std`-only).
        let step = span * e;
        let step = if step >= 0.0 { step + 0.5 } else { step - 0.5 } as i64;
        let at = i64::from(self.from_px) + step;
        (at.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32, false)
    }
}

/// The page index a pixel offset sits on (nearest page).
pub(crate) fn page_of(target_px: i32, view_w: i32) -> i32 {
    if view_w <= 0 {
        return 0;
    }
    (target_px + view_w / 2) / view_w
}

/// The pixel offset of page `i`, clamped to the scrollable extent.
pub(crate) fn page_target_px(page: i32, view_w: i32, max_x: i32) -> i32 {
    (page.max(0)).saturating_mul(view_w.max(0)).min(max_x.max(0))
}

/// The last page of a track `max_x` px past its first page, `view_w` px a page.
pub(crate) fn last_page(max_x: i32, view_w: i32) -> i32 {
    if view_w <= 0 || max_x <= 0 {
        return 0;
    }
    (max_x + view_w - 1) / view_w
}

#[cfg(test)]
mod tests;
