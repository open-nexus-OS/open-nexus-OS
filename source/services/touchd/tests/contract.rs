// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: `touchd` behavior-first host tests for bounded touch ingest and proof fixtures.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Stable
//! TEST_COVERAGE: readiness, synthetic fixture determinism, and reject taxonomy
//! ADR: docs/adr/0029-input-v1-host-core-architecture.md

use touch::{RawTouchSample, TouchBounds, TouchPhase, TouchTimestampNs};
use touchd::{SyntheticTouchMode, TouchDeviceId, TouchdError, TouchdService};

fn service(mode: SyntheticTouchMode) -> TouchdService {
    let bounds = TouchBounds::new(128, 128).expect("bounds");
    let mut service = TouchdService::new(bounds, mode);
    service.register_device(TouchDeviceId::new(9));
    service
}

#[test]
fn touch_service_reports_ready_once_registered() {
    let service = service(SyntheticTouchMode::Disabled);
    assert!(service.ready());
    assert!(service.recent_events().is_empty());
}

#[test]
fn test_reject_move_before_down() {
    let mut service = service(SyntheticTouchMode::Disabled);
    let err = service
        .ingest(RawTouchSample::new(
            TouchTimestampNs::new(1),
            12,
            24,
            TouchPhase::Move,
        ))
        .expect_err("must reject move before down");
    assert_eq!(err.code(), "touch.sequence.missing_down");
    assert_eq!(err, TouchdError::Normalize(touch::TouchError::MissingDown));
}

#[test]
fn test_reject_out_of_bounds_touch_sample() {
    let mut service = service(SyntheticTouchMode::Disabled);
    let err = service
        .ingest(RawTouchSample::new(
            TouchTimestampNs::new(2),
            200,
            24,
            TouchPhase::Down,
        ))
        .expect_err("must reject out-of-bounds sample");
    assert_eq!(err.code(), "touch.sample.out_of_bounds");
}

#[test]
fn test_synthetic_touch_sequence_is_deterministic() {
    let mut lhs = service(SyntheticTouchMode::ProofFixture);
    let mut rhs = service(SyntheticTouchMode::ProofFixture);

    let lhs_events = lhs.synthetic_sequence(1_000).expect("lhs synthetic");
    let rhs_events = rhs.synthetic_sequence(1_000).expect("rhs synthetic");

    assert_eq!(lhs_events, rhs_events);
    assert_eq!(lhs_events.len(), 3);
    assert_eq!(lhs_events[0].phase(), TouchPhase::Down);
    assert_eq!(lhs_events[1].phase(), TouchPhase::Move);
    assert_eq!(lhs_events[2].phase(), TouchPhase::Up);
    assert_eq!(lhs.recent_events(), lhs_events.as_slice());
}
