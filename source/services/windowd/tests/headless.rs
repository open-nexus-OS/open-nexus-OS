// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Window manager daemon headless and visible-present behavior tests.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: headless, visible-bootstrap, and visible SystemUI smoke tests
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
    let composed_frame = evidence.composed_frame.as_ref().expect("host composed frame");
    assert_eq!(composed_frame.width, windowd::VISIBLE_BOOTSTRAP_WIDTH);
    assert_eq!(composed_frame.height, windowd::VISIBLE_BOOTSTRAP_HEIGHT);
    assert_eq!(composed_frame.stride, windowd::VISIBLE_BOOTSTRAP_WIDTH * 4);
    assert_eq!(composed_frame.pixels[0..4], evidence.frame_source.pixels[0..4]);
    assert_eq!(evidence.first_present.seq.raw(), 1);
    assert_eq!(evidence.first_present.damage_rects, 1);
}
