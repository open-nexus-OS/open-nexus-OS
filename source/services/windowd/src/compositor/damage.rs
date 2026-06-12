// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Damage-rect helpers for the windowd compositor: cursor damage, effect inflation,
//! intersection tests, and flush error labels.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 2 unit tests (cursor_damage_rect)

use super::types::ProofBoxRect;
use super::{SOFT_PANEL_SHADOW_BLUR_RADIUS, SOFT_PANEL_SHADOW_OFFSET_Y};
use crate::assets;
use crate::error::WindowdError;
use crate::live_runtime::DamageRect;
use crate::smoke::VisibleBootstrapMode;

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
    let x0 = cursor_x.saturating_sub(assets::CURSOR_HOTSPOT_X);
    let y0 = cursor_y.saturating_sub(assets::CURSOR_HOTSPOT_Y);
    let x1 = x0.saturating_add(cursor_width as i32);
    let y1 = y0.saturating_add(cursor_height as i32);
    let start_x = x0.max(0).min(mode_width as i32) as u32;
    let start_y = y0.max(0).min(mode_height as i32) as u32;
    let end_x = x1.max(0).min(mode_width as i32) as u32;
    let end_y = y1.max(0).min(mode_height as i32) as u32;
    if end_x <= start_x || end_y <= start_y {
        return None;
    }
    Some(DamageRect {
        x: start_x,
        y: start_y,
        width: end_x - start_x,
        height: end_y - start_y,
    })
}

pub(crate) fn inflate_effect_rect(rect: ProofBoxRect, mode: VisibleBootstrapMode) -> DamageRect {
    let pad = SOFT_PANEL_SHADOW_BLUR_RADIUS
        .saturating_add(SOFT_PANEL_SHADOW_OFFSET_Y.unsigned_abs())
        .saturating_add(2);
    let x = rect.x.saturating_sub(pad);
    let y = rect.y.saturating_sub(pad);
    let end_x = rect
        .x
        .saturating_add(rect.width)
        .saturating_add(pad)
        .min(mode.width);
    let end_y = rect
        .y
        .saturating_add(rect.height)
        .saturating_add(pad)
        .min(mode.height);
    DamageRect {
        x,
        y,
        width: end_x.saturating_sub(x),
        height: end_y.saturating_sub(y),
    }
}

pub(crate) fn damage_rects_intersect(a: DamageRect, b: DamageRect) -> bool {
    a.x < b.end_x() && b.x < a.end_x() && a.y < b.end_y() && b.y < a.end_y()
}

pub(crate) fn flush_error_label(err: WindowdError) -> &'static str {
    match err {
        WindowdError::BufferLengthMismatch => "windowd: flush rows fail buffer-len",
        WindowdError::ArithmeticOverflow => "windowd: flush rows fail arith",
        _ => "windowd: flush rows fail",
    }
}
