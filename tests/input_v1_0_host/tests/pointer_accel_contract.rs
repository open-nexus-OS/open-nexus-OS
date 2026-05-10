// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Integration tests for monotonic bounded pointer acceleration behavior.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: 3 integration tests.
//!
//! TEST_SCOPE:
//!   - monotonic bounded accel curve behavior
//!   - extreme delta safety
//!   - invalid accel config rejects
//!
//! TEST_SCENARIOS:
//!   - pointer_accel_is_monotonic_and_bounded()
//!   - test_reject_* accel config rejects
//!
//! DEPENDENCIES:
//!   - `pointer_accel` crate config and curve application
//!
//! ADR: docs/adr/0029-input-v1-host-core-architecture.md

use pointer_accel::{PointerAccel, PointerAccelConfig};

#[test]
fn pointer_accel_is_monotonic_and_bounded() {
    let accel =
        PointerAccel::new(PointerAccelConfig::new(2, 2, 1, 12).expect("config")).expect("accel");

    let inputs = [0, 1, 2, 3, 4, 5, 6];
    let outputs: Vec<i32> =
        inputs.into_iter().map(|delta| accel.apply_axis(delta).expect("axis")).collect();

    assert_eq!(outputs[0], 0);
    assert!(outputs.windows(2).all(|pair| pair[0] <= pair[1]));
    assert!(outputs.iter().all(|value| *value <= 12));
    assert_eq!(accel.apply_axis(-6).expect("negative"), -10);
    assert_eq!(accel.apply_axis(50).expect("bounded"), 12);
    assert_eq!(accel.apply_axis(i32::MAX).expect("max"), 12);
    assert_eq!(accel.apply_axis(i32::MIN).expect("min"), -12);
}

#[test]
fn test_reject_pointer_accel_zero_denominator() {
    let err = PointerAccelConfig::new(2, 2, 0, 12).unwrap_err();
    assert_eq!(err.code(), "pointer_accel.denominator.invalid");
}

#[test]
fn test_reject_pointer_accel_non_increasing_bound() {
    let err = PointerAccelConfig::new(4, 2, 1, 3).unwrap_err();
    assert_eq!(err.code(), "pointer_accel.max_output.invalid");
}
