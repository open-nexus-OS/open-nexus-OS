// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Damage-rect helpers for the windowd compositor: cursor damage, effect inflation,
//! intersection tests, and flush error labels.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 2 unit tests (cursor_damage_rect)

use crate::assets;
use crate::error::WindowdError;

pub(crate) fn cursor_damage_rect(
    cursor_x: i32,
    cursor_y: i32,
    cursor_width: u32,
    cursor_height: u32,
    mode_width: u32,
    mode_height: u32,
) -> Option<DamageRect> {
    if cursor_width == 0 || cursor_height == 0 || mode_width == 0 || mode_height == 0 {
        return None;
    }
    // Conservative hotspot pad: shapes place the hotspot anywhere up to the
    // sprite center (TASK-0070 Phase 3 resize shapes are 16,16; the default
    // pointer 2,2). One worst-case rect avoids threading live shape state here.
    let x0 = cursor_x.saturating_sub(assets::CURSOR_RESIZE_HOTSPOT);
    let y0 = cursor_y.saturating_sub(assets::CURSOR_RESIZE_HOTSPOT);
    let x1 = x0.saturating_add(cursor_width as i32);
    let y1 = y0.saturating_add(cursor_height as i32);
    let start_x = x0.max(0).min(mode_width as i32) as u32;
    let start_y = y0.max(0).min(mode_height as i32) as u32;
    let end_x = x1.max(0).min(mode_width as i32) as u32;
    let end_y = y1.max(0).min(mode_height as i32) as u32;
    if end_x <= start_x || end_y <= start_y {
        return None;
    }
    Some(DamageRect { x: start_x, y: start_y, width: end_x - start_x, height: end_y - start_y })
}

pub(crate) fn flush_error_label(err: WindowdError) -> &'static str {
    match err {
        WindowdError::BufferLengthMismatch => "windowd: flush rows fail buffer-len",
        WindowdError::ArithmeticOverflow => "windowd: flush rows fail arith",
        _ => "windowd: flush rows fail",
    }
}

// ── Re-homed from the deleted `live_runtime.rs` (right-place move): the
// damage-rect model + premerge — pure damage/row accounting, i.e.
// compositor-service concerns. (The proof-panel layout hot-path row index
// was deleted with the proof panel itself.)
const DAMAGE_MERGE_AREA_PERCENT: u64 = 125;
const DAMAGE_MERGE_NEAR_GAP: u32 = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GlassQuality {
    High,
    Low,
    Opaque,
}

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

pub(crate) fn select_glass_quality(dirty_span_rows: u32) -> GlassQuality {
    if dirty_span_rows >= 320 {
        GlassQuality::Opaque
    } else if dirty_span_rows >= 160 {
        GlassQuality::Low
    } else {
        GlassQuality::High
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glass_quality_degrades_deterministically() {
        assert_eq!(select_glass_quality(32), GlassQuality::High);
        assert_eq!(select_glass_quality(160), GlassQuality::Low);
        assert_eq!(select_glass_quality(400), GlassQuality::Opaque);
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
            DamageRect { x: 10, y: 10, width: 20, height: 20 },
            DamageRect { x: 25, y: 10, width: 20, height: 20 },
            DamageRect { x: 400, y: 400, width: 20, height: 20 },
        ];
        let count = premerge_damage_rects(&mut rects, 3);
        assert_eq!(count, 2);
        assert!(rects[..count].iter().any(|rect| rect.width == 35 && rect.height == 20));
        assert!(rects[..count].iter().any(|rect| rect.x == 400 && rect.y == 400));
    }
}
