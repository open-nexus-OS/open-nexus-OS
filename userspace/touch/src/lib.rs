// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: TASK-0252 transport-neutral touch normalization primitives.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: No direct tests (covered by 4 integration tests in `tests/input_v1_0_host/tests/touch_contract.rs`).
//! ADR: docs/adr/0029-input-v1-host-core-architecture.md

//! CONTEXT: TASK-0252 transport-neutral touch normalization primitives.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Stable for TASK-0252 host proof floor
//! TEST_COVERAGE: Integration coverage in `tests/input_v1_0_host/tests/touch_contract.rs`.
//! ADR: docs/rfcs/RFC-0052-input-v1_0a-host-hid-touch-keymaps-repeat-accel-contract.md

#![forbid(unsafe_code)]

mod error;
mod normalizer;
mod types;

pub use error::TouchError;
pub use normalizer::TouchNormalizer;
pub use types::{
    RawTouchSample, TouchBounds, TouchEvent, TouchPhase, TouchTimestampNs, TouchX, TouchY,
};
