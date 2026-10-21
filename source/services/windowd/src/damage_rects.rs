// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The compositor's damage-rect model — pure, host-TESTED, consumed by the OS-only
//! compositor runtime: the rect, its union, the pre-merge of near rects before a present, and
//! the bounded pending list a present collects its damage into (TASK-0335: `windowd: loop …
//! damage(rects= queued_px= flushed_px=)` says what the joins cost against what was queued).
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: unit tests below (the union, the pre-merge, the full list never drops a rect)
//! INVARIANTS: a queued rect is never lost — a dropped rect is a stale pixel on the screen; the
//!   pending list never exceeds `PENDING_DAMAGE_RECTS`.

use alloc::vec::Vec;

const DAMAGE_MERGE_AREA_PERCENT: u64 = 125;
const DAMAGE_MERGE_NEAR_GAP: u32 = 64;

/// The most damage rects a present keeps apart before they join.
pub(crate) const PENDING_DAMAGE_RECTS: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DamageRect {
    pub(crate) x: u32,
    pub(crate) y: u32,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

impl DamageRect {
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

    const fn touches(self, other: Self) -> bool {
        other.x <= self.end_x()
            && other.end_x() >= self.x
            && other.y <= self.end_y()
            && other.end_y() >= self.y
    }
}

fn damage_area(rect: DamageRect) -> u64 {
    u64::from(rect.width).saturating_mul(u64::from(rect.height))
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
    let merged = a.merge(b);
    let merged_area = damage_area(merged);
    let source_area = damage_area(a).saturating_add(damage_area(b));
    merged_area.saturating_mul(100) <= source_area.saturating_mul(DAMAGE_MERGE_AREA_PERCENT).max(1)
}

/// Join near rects whose union grows little; returns the count that remain.
pub(crate) fn premerge_damage_rects(rects: &mut [DamageRect], mut count: usize) -> usize {
    let mut idx = 0;
    while idx < count {
        let mut scan = idx + 1;
        while scan < count {
            if should_premerge_damage(rects[idx], rects[scan]) {
                rects[idx] = rects[idx].merge(rects[scan]);
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

/// Queue a damage rect into the bounded pending list: it joins the first rect it touches; a
/// rect touching none takes a free place, or — the list full — joins the rect whose union grows
/// the least. A rect is never dropped.
pub(crate) fn queue_damage_rect(pending: &mut Vec<DamageRect>, rect: DamageRect) {
    for existing in pending.iter_mut() {
        if existing.touches(rect) {
            *existing = existing.merge(rect);
            return;
        }
    }
    if pending.len() < PENDING_DAMAGE_RECTS {
        pending.push(rect);
        return;
    }
    let mut best = 0usize;
    let mut best_growth = u64::MAX;
    for (i, existing) in pending.iter().enumerate() {
        let growth = damage_area(existing.merge(rect)).saturating_sub(damage_area(*existing));
        if growth < best_growth {
            best_growth = growth;
            best = i;
        }
    }
    let merged = pending[best].merge(rect);
    pending[best] = merged;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn damage_rect_merge_bounds_small_target_updates() {
        let left = DamageRect { x: 10, y: 20, width: 30, height: 40 };
        let right = DamageRect { x: 32, y: 10, width: 20, height: 12 };
        assert_eq!(left.merge(right), DamageRect { x: 10, y: 10, width: 42, height: 50 });
    }

    #[test]
    fn damage_premerge_merges_only_bounded_area_growth() {
        let mut rects = [
            DamageRect { x: 10, y: 10, width: 20, height: 20 },
            DamageRect { x: 25, y: 10, width: 20, height: 20 },
            DamageRect { x: 400, y: 400, width: 20, height: 20 },
        ];
        let count = premerge_damage_rects(&mut rects, 3);
        assert_eq!(count, 2);
        assert!(rects[..count].iter().any(|rect| rect.width == 35 && rect.height == 20));
        assert!(rects[..count].iter().any(|rect| rect.x == 400 && rect.y == 400));
    }

    #[test]
    fn a_touching_rect_joins_its_neighbour_in_place() {
        let mut pending = Vec::new();
        queue_damage_rect(&mut pending, DamageRect { x: 0, y: 0, width: 10, height: 10 });
        queue_damage_rect(&mut pending, DamageRect { x: 5, y: 5, width: 10, height: 10 });
        assert_eq!(pending, [DamageRect { x: 0, y: 0, width: 15, height: 15 }]);
    }

    #[test]
    fn test_reject_dropping_a_fifth_disjoint_damage_rect() {
        let mut pending = Vec::new();
        for i in 0..PENDING_DAMAGE_RECTS as u32 {
            queue_damage_rect(&mut pending, DamageRect { x: 100 * i, y: 0, width: 10, height: 10 });
        }
        assert_eq!(pending.len(), PENDING_DAMAGE_RECTS);
        // Touches none and the list is full: it joins the rect whose union grows the least
        // (the one at x=300, 20 px away), it never vanishes.
        queue_damage_rect(&mut pending, DamageRect { x: 330, y: 0, width: 10, height: 10 });
        assert_eq!(pending.len(), PENDING_DAMAGE_RECTS);
        let holder = pending.iter().find(|r| r.x == 300).expect("the nearest rect grew");
        assert_eq!(holder.end_x(), 340);
        assert!(pending.iter().filter(|r| r.x != 300).all(|r| r.width == 10));
    }
}
