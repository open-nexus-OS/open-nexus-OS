// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Damage-rect helpers for the windowd compositor: cursor damage, effect inflation,
//! intersection tests, and flush error labels.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: the rect model and its queue in `crate::damage_rects` (host); glass quality
//!   below (OS-only, with the compositor)

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

pub(crate) use crate::damage_rects::{premerge_damage_rects, queue_damage_rect, DamageRect};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GlassQuality {
    High,
    Low,
    Opaque,
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
}
