// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Window manager daemon headless behavior tests.
//! OWNERS: @ui @runtime
//! STATUS: Done
//! TEST_COVERAGE: real TASK-0055 smoke behavior; checksum scaffold removed.

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
