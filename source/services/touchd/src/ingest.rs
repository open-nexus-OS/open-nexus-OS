// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Thin ingest adapters over the RFC-0052 touch normalization authority.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `cargo test -p touchd -- --nocapture`
//! ADR: docs/adr/0029-input-v1-host-core-architecture.md

extern crate alloc;

use crate::TouchdError;
use alloc::vec;
use alloc::vec::Vec;
use touch::{RawTouchSample, TouchEvent, TouchNormalizer};

pub(crate) fn normalize_sample(
    normalizer: &mut TouchNormalizer,
    sample: RawTouchSample,
) -> Result<TouchEvent, TouchdError> {
    normalizer.normalize(sample).map_err(TouchdError::from)
}

pub(crate) fn synthetic_fixture(
    start_ns: u64,
    x0: u32,
    y0: u32,
    x1: u32,
    y1: u32,
) -> Vec<RawTouchSample> {
    vec![
        RawTouchSample::new(touch::TouchTimestampNs::new(start_ns), x0, y0, touch::TouchPhase::Down),
        RawTouchSample::new(
            touch::TouchTimestampNs::new(start_ns + 1_000_000),
            x1,
            y1,
            touch::TouchPhase::Move,
        ),
        RawTouchSample::new(
            touch::TouchTimestampNs::new(start_ns + 2_000_000),
            x1,
            y1,
            touch::TouchPhase::Up,
        ),
    ]
}
