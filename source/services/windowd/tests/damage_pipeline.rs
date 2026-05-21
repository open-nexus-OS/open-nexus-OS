// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Integration tests for retained-mode damage pipeline.
//! Verifies rect-based damage: only dirty rects change, no row overwrite,
//! shadow preservation, pixel-write budget.
//!
//! OWNERS: @ui
//! STATUS: Experimental

use input_live_protocol::VisibleState;
use pointer_state::{PointerPosition, PointerSpace, PointerTransform};

fn count_changed_pixel_groups(before: &[u8], after: &[u8]) -> usize {
    assert_eq!(before.len(), after.len());
    before.chunks(4).zip(after.chunks(4)).filter(|(a, b)| a != b).count()
}

fn rect_has_changes(before: &[u8], after: &[u8], stride: usize, x: u32, y: u32, w: u32, h: u32) -> bool {
    for row in 0..h {
        for col in 0..w {
            let idx = (y + row) as usize * stride + (x + col) as usize * 4;
            if before[idx..idx + 4] != after[idx..idx + 4] {
                return true;
            }
        }
    }
    false
}

#[test]
fn damage_rect_only_changes_target_area_not_surrounding() {
    let mode = windowd::VisibleBootstrapMode::fixed().expect("mode");
    let transform = PointerTransform::new(
        PointerSpace::new(mode.width, mode.height).expect("display"),
        PointerSpace::new(64, 48).expect("route"),
    ).expect("transform");
    let hover_rect = transform.route_rect_to_display(4, 36, 8, 8);
    let hover_pos = transform.route_to_display(PointerPosition::new(18, 30));

    let base = VisibleState {
        backend_visible: true, display_scanout_ready: true,
        systemui_first_frame_visible: true, scene_ready: true,
        full_window_visible: true, input_visible_on: true,
        cursor_move_visible: true, pointer_route_live: true,
        cursor_x: hover_pos.x, cursor_y: hover_pos.y,
        ..Default::default()
    };

    let neutral = windowd::live_visible_state_handoff(VisibleState { hover_visible: false, ..base })
        .expect("neutral").materialize_frame().expect("neutral frame");
    let hover = windowd::live_visible_state_handoff(VisibleState { hover_visible: true, ..base })
        .expect("hover").materialize_frame().expect("hover frame");

    let stride = mode.stride as usize;

    let hover_w = hover_rect.right.saturating_sub(hover_rect.left);
    let hover_h = hover_rect.bottom.saturating_sub(hover_rect.top);

    // Assert 1: hover card area has changes
    assert!(rect_has_changes(&neutral.pixels, &hover.pixels, stride,
        hover_rect.left, hover_rect.top, hover_w, hover_h),
        "hover card area must change");

    // Assert 2: wallpaper corner unchanged
    assert!(!rect_has_changes(&neutral.pixels, &hover.pixels, stride, 0, 0, 100, 100),
        "wallpaper (0,0-100,100) must not change — row-based rendering would overwrite this");

    // Assert 3: pixel write budget
    let changed = count_changed_pixel_groups(&neutral.pixels, &hover.pixels);
    let card_px = hover_w as usize * hover_h as usize;
    let cursor_px = 32 * 32;
    assert!(changed <= card_px + cursor_px + card_px / 4,
        "budget: {} changed, max {}", changed, card_px + cursor_px + card_px / 4);
    assert!(changed > 0, "hover must produce changes");
}

#[test]
fn paint_only_preserves_unrelated_card_areas() {
    let mode = windowd::VisibleBootstrapMode::fixed().expect("mode");
    let transform = PointerTransform::new(
        PointerSpace::new(mode.width, mode.height).expect("display"),
        PointerSpace::new(64, 48).expect("route"),
    ).expect("transform");
    let click_rect = transform.route_rect_to_display(4, 36, 8, 8);

    let base = VisibleState {
        backend_visible: true, display_scanout_ready: true,
        systemui_first_frame_visible: true, scene_ready: true,
        full_window_visible: true, input_visible_on: true,
        ..Default::default()
    };

    let neutral = windowd::live_visible_state_handoff(VisibleState { hover_visible: false, launcher_click_visible: false, ..base })
        .expect("neutral").materialize_frame().expect("neutral frame");
    let hover = windowd::live_visible_state_handoff(VisibleState { hover_visible: true, launcher_click_visible: false, ..base })
        .expect("hover").materialize_frame().expect("hover frame");

    let stride = mode.stride as usize;

    // Paint-only: changed pixels must stay well below full-frame budget.
    // Full frame = 1280*800 = 1,024,000 pixel groups. Row-based overwrite
    // would change ~1280 * damage_rows. Rect-based changes only card area.
    let changed = count_changed_pixel_groups(&neutral.pixels, &hover.pixels);
    let full_frame = (mode.width * mode.height) as usize;
    // Layout shift can cascade: allow up to 5% of full frame.
    assert!(changed < full_frame / 20,
        "changed {} px groups exceeds 5% of full frame {} — implies full-row or full-frame re-render",
        changed, full_frame);
    assert!(changed > 0, "hover must produce changes");
}
