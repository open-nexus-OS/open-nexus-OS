// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Window manager daemon headless and visible-present behavior tests.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: headless, visible-bootstrap, visible SystemUI, and visible-input smoke tests
//! ADR: docs/adr/0028-windowd-surface-present-and-visible-bootstrap-architecture.md

#[test]
fn headless_smoke_produces_present_and_resize_evidence() {
    let evidence = match windowd::run_headless_ui_smoke() {
        Ok(evidence) => evidence,
        Err(err) => panic!("headless smoke failed: {err:?}"),
    };
    assert!(evidence.ready);
    assert!(evidence.systemui_loaded);
    assert!(evidence.launcher_first_frame);
    assert!(evidence.resize_ok);
    assert_eq!(evidence.first_present.seq.raw(), 1);
    assert_eq!(evidence.first_present.damage_rects, 1);
}

#[test]
fn test_reject_marker_before_present_state() {
    assert_eq!(
        windowd::marker_postflight_ready(None),
        Err(windowd::WindowdError::MarkerBeforePresentState)
    );
}

#[test]
fn visible_bootstrap_smoke_produces_mode_and_present_evidence() {
    let evidence = match windowd::run_visible_bootstrap_smoke() {
        Ok(evidence) => evidence,
        Err(err) => panic!("visible bootstrap smoke failed: {err:?}"),
    };
    assert!(evidence.ready);
    assert_eq!(evidence.mode.width, windowd::VISIBLE_BOOTSTRAP_WIDTH);
    assert_eq!(evidence.mode.height, windowd::VISIBLE_BOOTSTRAP_HEIGHT);
    assert_eq!(evidence.seed_surface.width, 64);
    assert_eq!(evidence.seed_surface.height, 48);
    assert_eq!(evidence.first_present.seq.raw(), 1);
    assert_eq!(evidence.first_present.damage_rects, 1);
}

#[test]
fn visible_systemui_smoke_produces_first_frame_present_evidence() {
    let evidence = match windowd::run_visible_systemui_smoke() {
        Ok(evidence) => evidence,
        Err(err) => panic!("visible systemui smoke failed: {err:?}"),
    };
    assert!(evidence.ready);
    assert!(evidence.backend_visible);
    assert!(evidence.systemui_first_frame);
    assert_eq!(evidence.mode.width, windowd::VISIBLE_BOOTSTRAP_WIDTH);
    assert_eq!(evidence.mode.height, windowd::VISIBLE_BOOTSTRAP_HEIGHT);
    assert_eq!(evidence.frame_source.width, 160);
    assert_eq!(evidence.frame_source.height, 100);
    let composed_frame = evidence
        .composed_frame
        .as_ref()
        .expect("host composed frame");
    assert_eq!(composed_frame.width, windowd::VISIBLE_BOOTSTRAP_WIDTH);
    assert_eq!(composed_frame.height, windowd::VISIBLE_BOOTSTRAP_HEIGHT);
    assert_eq!(composed_frame.stride, windowd::VISIBLE_BOOTSTRAP_WIDTH * 4);
    assert_eq!(
        composed_frame.pixels[0..4],
        evidence.frame_source.pixels[0..4]
    );
    assert_eq!(evidence.first_present.seq.raw(), 1);
    assert_eq!(evidence.first_present.damage_rects, 1);
}

#[test]
fn visible_input_smoke_produces_cursor_focus_and_click_evidence() {
    let evidence = match windowd::run_visible_input_smoke() {
        Ok(evidence) => evidence,
        Err(err) => panic!("visible input smoke failed: {err:?}"),
    };
    assert!(evidence.input_visible_on);
    assert!(evidence.cursor_move_visible);
    assert!(evidence.hover_visible);
    assert!(evidence.focus_visible);
    assert!(evidence.launcher_click_visible);
    assert_eq!(evidence.focused_surface.raw(), 1);
    assert_eq!(evidence.cursor_start_position.x, 12);
    assert_eq!(evidence.cursor_start_position.y, 12);
    assert_eq!(evidence.cursor_position.x, 36);
    assert_eq!(evidence.cursor_position.y, 28);
    assert_eq!(evidence.scheduled_present.damage_rects, 1);
    let mode = windowd::VisibleBootstrapMode::fixed().expect("fixed visible mode");
    let mut row = vec![0u8; mode.stride as usize];
    evidence
        .copy_cursor_row(mode, 200, &mut row)
        .expect("scaled cursor row");
    let scaled_start_cursor_x = 240usize;
    let cursor = scaled_start_cursor_x * 4;
    assert_eq!(row[cursor..cursor + 4], windowd::VISIBLE_CURSOR_BGRA);
    evidence
        .copy_hover_row(mode, 140, &mut row)
        .expect("scaled hover row");
    let scaled_hover_x = 160usize;
    let hover = scaled_hover_x * 4;
    assert_eq!(row[hover..hover + 4], windowd::VISIBLE_HOVER_BGRA);
    evidence
        .copy_composed_row(mode, 470, &mut row)
        .expect("scaled visible input row");
    let scaled_cursor_x = 720usize;
    let cursor = scaled_cursor_x * 4;
    assert_eq!(row[cursor..cursor + 4], windowd::VISIBLE_CURSOR_BGRA);
}
