// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Window manager daemon headless and visible-present behavior tests.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: headless, visible-bootstrap, visible SystemUI, and visible-input smoke tests
//! ADR: docs/adr/0028-windowd-surface-present-and-visible-bootstrap-architecture.md

use pointer_state::{PointerPosition, PointerSpace, PointerTransform};

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
    let composed_frame = evidence.composed_frame.as_ref().expect("host composed frame");
    assert_eq!(composed_frame.width, windowd::VISIBLE_BOOTSTRAP_WIDTH);
    assert_eq!(composed_frame.height, windowd::VISIBLE_BOOTSTRAP_HEIGHT);
    assert_eq!(composed_frame.stride, windowd::VISIBLE_BOOTSTRAP_WIDTH * 4);
    assert_eq!(composed_frame.pixels[0..4], evidence.frame_source.pixels[0..4]);
    assert_eq!(evidence.first_present.seq.raw(), 1);
    assert_eq!(evidence.first_present.damage_rects, 1);
}

#[test]
fn display_bootstrap_handoff_keeps_windowd_as_scene_authority() {
    let handoff = windowd::bootstrap_display_handoff().expect("bootstrap handoff");
    let frame = handoff.materialize_frame().expect("bootstrap materialized frame");

    assert_eq!(handoff.mode.width, windowd::VISIBLE_BOOTSTRAP_WIDTH);
    assert_eq!(handoff.mode.height, windowd::VISIBLE_BOOTSTRAP_HEIGHT);
    assert_eq!(handoff.damage_rects, 1);
    assert!(handoff.backend_visible);
    assert!(handoff.scanout_ready);
    assert!(handoff.systemui_first_frame_visible);
    assert_eq!(frame.width, handoff.mode.width);
    assert_eq!(frame.height, handoff.mode.height);
    assert_eq!(frame.stride, handoff.mode.stride);
    assert!(!frame.pixels.is_empty());
}

#[test]
fn live_visible_state_handoff_composes_cursor_click_and_keyboard_targets() {
    let mode = windowd::VisibleBootstrapMode::fixed().expect("fixed visible mode");
    let transform = PointerTransform::new(
        PointerSpace::new(mode.width, mode.height).expect("display"),
        PointerSpace::new(64, 48).expect("route"),
    )
    .expect("transform");
    let cursor = transform.route_to_display(PointerPosition::new(8, 40));
    let click_rect = transform.route_rect_to_display(4, 36, 8, 8);
    let keyboard_rect = transform.route_rect_to_display(52, 18, 8, 8);
    let handoff = windowd::live_visible_state_handoff(input_live_protocol::VisibleState {
        backend_visible: true,
        display_scanout_ready: true,
        systemui_first_frame_visible: true,
        scene_ready: true,
        full_window_visible: true,
        click_target_visible: true,
        keyboard_target_visible: true,
        input_visible_on: true,
        cursor_move_visible: true,
        hover_visible: true,
        focus_visible: true,
        launcher_click_visible: true,
        keyboard_visible: true,
        pointer_route_live: true,
        keyboard_route_live: true,
        cursor_x: cursor.x,
        cursor_y: cursor.y,
        ..Default::default()
    })
    .expect("live handoff");
    let frame = handoff.materialize_frame().expect("live materialized frame");

    let stride = handoff.mode.stride as usize;
    let cursor = cursor.y as usize * stride + cursor.x as usize * 4;
    let click = click_rect.top as usize * stride + click_rect.left as usize * 4;
    let keyboard = keyboard_rect.top as usize * stride + keyboard_rect.left as usize * 4;

    assert_eq!(frame.pixels[cursor..cursor + 4], windowd::VISIBLE_CURSOR_BGRA);
    assert_eq!(frame.pixels[click..click + 4], windowd::VISIBLE_INPUT_CLICK_BGRA);
    assert_eq!(frame.pixels[keyboard..keyboard + 4], windowd::VISIBLE_INPUT_KEYBOARD_BGRA);
}

#[test]
fn live_visible_state_handoff_composes_transient_wheel_direction_indicators() {
    let mode = windowd::VisibleBootstrapMode::fixed().expect("fixed visible mode");
    let transform = PointerTransform::new(
        PointerSpace::new(mode.width, mode.height).expect("display"),
        PointerSpace::new(64, 48).expect("route"),
    )
    .expect("transform");
    let up_indicator = transform.route_to_display(PointerPosition::new(17, 37));
    let down_indicator = transform.route_to_display(PointerPosition::new(17, 42));

    let up_frame = windowd::live_visible_state_handoff(input_live_protocol::VisibleState {
        backend_visible: true,
        display_scanout_ready: true,
        systemui_first_frame_visible: true,
        scene_ready: true,
        wheel_up_visible: true,
        cursor_x: 0,
        cursor_y: 0,
        ..Default::default()
    })
    .expect("up handoff")
    .materialize_frame()
    .expect("up frame");
    let down_frame = windowd::live_visible_state_handoff(input_live_protocol::VisibleState {
        backend_visible: true,
        display_scanout_ready: true,
        systemui_first_frame_visible: true,
        scene_ready: true,
        wheel_down_visible: true,
        cursor_x: 0,
        cursor_y: 0,
        ..Default::default()
    })
    .expect("down handoff")
    .materialize_frame()
    .expect("down frame");

    let stride = mode.stride as usize;
    let up_idx = up_indicator.y as usize * stride + up_indicator.x as usize * 4;
    let down_idx = down_indicator.y as usize * stride + down_indicator.x as usize * 4;

    assert_eq!(up_frame.pixels[up_idx..up_idx + 4], windowd::VISIBLE_INPUT_WHEEL_ACTIVE_BGRA);
    assert_eq!(up_frame.pixels[down_idx..down_idx + 4], windowd::VISIBLE_INPUT_WHEEL_IDLE_BGRA);
    assert_eq!(down_frame.pixels[up_idx..up_idx + 4], windowd::VISIBLE_INPUT_WHEEL_IDLE_BGRA);
    assert_eq!(down_frame.pixels[down_idx..down_idx + 4], windowd::VISIBLE_INPUT_WHEEL_ACTIVE_BGRA);
}

#[test]
fn live_visible_cursor_coordinates_preserve_screen_direction_when_scaled() {
    let mode = windowd::VisibleBootstrapMode::fixed().expect("fixed visible mode");
    let transform = PointerTransform::new(
        PointerSpace::new(mode.width, mode.height).expect("display"),
        PointerSpace::new(64, 48).expect("route"),
    )
    .expect("transform");
    let mut row = vec![0u8; mode.stride as usize];
    let base_cursor = transform.route_to_display(PointerPosition::new(8, 12));
    let base = input_live_protocol::VisibleState {
        backend_visible: true,
        display_scanout_ready: true,
        systemui_first_frame_visible: true,
        scene_ready: true,
        cursor_x: base_cursor.x,
        cursor_y: base_cursor.y,
        ..Default::default()
    };

    windowd::copy_live_visible_row(base, mode, base_cursor.y as u32, &mut row)
        .expect("base cursor row");
    assert_eq!(
        row[base_cursor.x as usize * 4..base_cursor.x as usize * 4 + 4],
        windowd::VISIBLE_CURSOR_BGRA
    );

    row.fill(0);
    let moved_cursor = transform.route_to_display(PointerPosition::new(9, 13));
    let moved_down_right = input_live_protocol::VisibleState {
        cursor_x: moved_cursor.x,
        cursor_y: moved_cursor.y,
        ..base
    };
    windowd::copy_live_visible_row(moved_down_right, mode, moved_cursor.y as u32, &mut row)
        .expect("moved cursor row");
    assert_eq!(
        row[moved_cursor.x as usize * 4..moved_cursor.x as usize * 4 + 4],
        windowd::VISIBLE_CURSOR_BGRA
    );
}

#[test]
fn visible_input_smoke_produces_cursor_focus_and_click_evidence() {
    let evidence = match windowd::run_visible_input_smoke() {
        Ok(evidence) => evidence,
        Err(err) => panic!("visible input smoke failed: {err:?}"),
    };
    assert!(evidence.input_visible_on);
    assert!(evidence.full_window_visible);
    assert!(evidence.cursor_move_visible);
    assert!(evidence.hover_visible);
    assert!(evidence.focus_visible);
    assert!(evidence.launcher_click_visible);
    assert!(evidence.keyboard_visible);
    assert_eq!(evidence.focused_surface.raw(), 1);
    assert_eq!(evidence.cursor_start_position.x, 24);
    assert_eq!(evidence.cursor_start_position.y, 12);
    assert_eq!(evidence.cursor_position.x, 8);
    assert_eq!(evidence.cursor_position.y, 40);
    assert_eq!(evidence.scheduled_present.damage_rects, 1);
    let mode = windowd::VisibleBootstrapMode::fixed().expect("fixed visible mode");
    let mut row = vec![0u8; mode.stride as usize];
    evidence.copy_cursor_row(mode, 200, &mut row).expect("scaled cursor row");
    let scaled_start_cursor_x = 480usize;
    let cursor = scaled_start_cursor_x * 4;
    assert_eq!(row[cursor..cursor + 4], windowd::VISIBLE_CURSOR_BGRA);
    evidence.copy_hover_row(mode, 670, &mut row).expect("scaled hover row");
    let scaled_hover_x = 160usize;
    let hover = scaled_hover_x * 4;
    assert_eq!(row[hover..hover + 4], windowd::VISIBLE_CURSOR_BGRA);
    evidence.copy_composed_row(mode, 620, &mut row).expect("scaled visible input row");
    let scaled_click_x = 100usize;
    let click = scaled_click_x * 4;
    assert_eq!(row[click..click + 4], windowd::VISIBLE_INPUT_CLICK_BGRA);
    evidence.copy_keyboard_row(mode, 320, &mut row).expect("scaled keyboard row");
    let scaled_keyboard_x = 1060usize;
    let keyboard = scaled_keyboard_x * 4;
    assert_eq!(row[keyboard..keyboard + 4], windowd::VISIBLE_INPUT_KEYBOARD_BGRA);
}
