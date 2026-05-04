// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Stable reject taxonomy for touch bounds and lifecycle violations.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: No direct tests (covered by 4 integration tests in `tests/input_v1_0_host/tests/touch_contract.rs`).
//! ADR: docs/adr/0029-input-v1-host-core-architecture.md

use core::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TouchError {
    InvalidBounds,
    MissingDown,
    DuplicateDown,
    OutOfBounds { x: u32, y: u32 },
}

impl TouchError {
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::InvalidBounds => "touch.bounds.invalid",
            Self::MissingDown => "touch.sequence.missing_down",
            Self::DuplicateDown => "touch.sequence.duplicate_down",
            Self::OutOfBounds { .. } => "touch.sample.out_of_bounds",
        }
    }
}

impl fmt::Display for TouchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidBounds => f.write_str("touch bounds must be non-zero"),
            Self::MissingDown => f.write_str("touch sequence requires down before move/up"),
            Self::DuplicateDown => f.write_str("touch sequence received duplicate down"),
            Self::OutOfBounds { x, y } => write!(f, "touch sample out of bounds: ({x}, {y})"),
        }
    }
}

impl std::error::Error for TouchError {}
