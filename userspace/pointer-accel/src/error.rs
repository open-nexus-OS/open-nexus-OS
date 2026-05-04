// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Stable reject taxonomy for pointer acceleration configuration.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: No direct tests (covered by 3 integration tests in `tests/input_v1_0_host/tests/pointer_accel_contract.rs`).
//! ADR: docs/adr/0029-input-v1-host-core-architecture.md

use core::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PointerAccelError {
    InvalidThreshold,
    InvalidNumerator,
    InvalidDenominator,
    InvalidMaxOutput,
}

impl PointerAccelError {
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::InvalidThreshold => "pointer_accel.threshold.invalid",
            Self::InvalidNumerator => "pointer_accel.numerator.invalid",
            Self::InvalidDenominator => "pointer_accel.denominator.invalid",
            Self::InvalidMaxOutput => "pointer_accel.max_output.invalid",
        }
    }
}

impl fmt::Display for PointerAccelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidThreshold => f.write_str("pointer accel threshold must be non-negative"),
            Self::InvalidNumerator => f.write_str("pointer accel numerator must be non-zero"),
            Self::InvalidDenominator => f.write_str("pointer accel denominator must be non-zero"),
            Self::InvalidMaxOutput => f.write_str("pointer accel max output must exceed threshold"),
        }
    }
}

impl std::error::Error for PointerAccelError {}
