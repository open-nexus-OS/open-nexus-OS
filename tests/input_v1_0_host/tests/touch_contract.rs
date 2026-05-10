// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Integration tests for touch normalization lifecycle ordering and rejects.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: 4 integration tests.
//!
//! TEST_SCOPE:
//!   - deterministic down/move/up ordering
//!   - lifecycle reject behavior
//!   - bounds reject behavior
//!
//! TEST_SCENARIOS:
//!   - touch_sequence_preserves_down_move_up_order()
//!   - test_reject_* touch lifecycle and bounds rejects
//!
//! DEPENDENCIES:
//!   - `touch` crate normalizer and typed samples
//!
//! ADR: docs/adr/0029-input-v1-host-core-architecture.md

use touch::{
    RawTouchSample, TouchBounds, TouchEvent, TouchNormalizer, TouchPhase, TouchTimestampNs,
};

fn ts(ns: u64) -> TouchTimestampNs {
    TouchTimestampNs::new(ns)
}

fn event_tuple(event: TouchEvent) -> (TouchPhase, u32, u32) {
    (event.phase(), event.x().raw(), event.y().raw())
}

#[test]
fn touch_sequence_preserves_down_move_up_order() {
    let bounds = TouchBounds::new(1920, 1080).expect("bounds");
    let mut normalizer = TouchNormalizer::new(bounds);

    let down = normalizer
        .normalize(RawTouchSample::new(ts(10), 100, 200, TouchPhase::Down))
        .expect("down");
    let move_one = normalizer
        .normalize(RawTouchSample::new(ts(20), 120, 240, TouchPhase::Move))
        .expect("move one");
    let move_two = normalizer
        .normalize(RawTouchSample::new(ts(30), 128, 256, TouchPhase::Move))
        .expect("move two");
    let up =
        normalizer.normalize(RawTouchSample::new(ts(40), 128, 256, TouchPhase::Up)).expect("up");

    assert_eq!(event_tuple(down), (TouchPhase::Down, 100, 200));
    assert_eq!(event_tuple(move_one), (TouchPhase::Move, 120, 240));
    assert_eq!(event_tuple(move_two), (TouchPhase::Move, 128, 256));
    assert_eq!(event_tuple(up), (TouchPhase::Up, 128, 256));
}

#[test]
fn test_reject_move_before_down() {
    let bounds = TouchBounds::new(640, 480).expect("bounds");
    let mut normalizer = TouchNormalizer::new(bounds);
    let err =
        normalizer.normalize(RawTouchSample::new(ts(50), 10, 20, TouchPhase::Move)).unwrap_err();
    assert_eq!(err.code(), "touch.sequence.missing_down");
}

#[test]
fn test_reject_duplicate_down_without_up() {
    let bounds = TouchBounds::new(640, 480).expect("bounds");
    let mut normalizer = TouchNormalizer::new(bounds);
    normalizer
        .normalize(RawTouchSample::new(ts(60), 20, 30, TouchPhase::Down))
        .expect("first down");

    let err =
        normalizer.normalize(RawTouchSample::new(ts(70), 22, 32, TouchPhase::Down)).unwrap_err();
    assert_eq!(err.code(), "touch.sequence.duplicate_down");
}

#[test]
fn test_reject_touch_out_of_bounds() {
    let bounds = TouchBounds::new(640, 480).expect("bounds");
    let mut normalizer = TouchNormalizer::new(bounds);
    let err =
        normalizer.normalize(RawTouchSample::new(ts(80), 641, 20, TouchPhase::Down)).unwrap_err();
    assert_eq!(err.code(), "touch.sample.out_of_bounds");
}
