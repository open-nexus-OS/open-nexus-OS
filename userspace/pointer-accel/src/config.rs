// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Pointer acceleration config and bounded curve application for host input.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: No direct tests (covered by 3 integration tests in `tests/input_v1_0_host/tests/pointer_accel_contract.rs`).
//! ADR: docs/adr/0029-input-v1-host-core-architecture.md

use crate::PointerAccelError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PointerAccelConfig {
    threshold: i32,
    numerator: i32,
    denominator: i32,
    max_output: i32,
}

impl PointerAccelConfig {
    pub fn new(
        threshold: i32,
        numerator: i32,
        denominator: i32,
        max_output: i32,
    ) -> Result<Self, PointerAccelError> {
        if threshold < 0 {
            return Err(PointerAccelError::InvalidThreshold);
        }
        if numerator <= 0 {
            return Err(PointerAccelError::InvalidNumerator);
        }
        if denominator <= 0 {
            return Err(PointerAccelError::InvalidDenominator);
        }
        if max_output <= threshold {
            return Err(PointerAccelError::InvalidMaxOutput);
        }
        Ok(Self { threshold, numerator, denominator, max_output })
    }

    #[must_use]
    pub const fn threshold(self) -> i32 {
        self.threshold
    }

    #[must_use]
    pub const fn numerator(self) -> i32 {
        self.numerator
    }

    #[must_use]
    pub const fn denominator(self) -> i32 {
        self.denominator
    }

    #[must_use]
    pub const fn max_output(self) -> i32 {
        self.max_output
    }
}

#[derive(Debug, Clone, Copy)]
pub struct PointerAccel {
    config: PointerAccelConfig,
}

impl PointerAccel {
    pub fn new(config: PointerAccelConfig) -> Result<Self, PointerAccelError> {
        if config.denominator() <= 0 {
            return Err(PointerAccelError::InvalidDenominator);
        }
        Ok(Self { config })
    }

    pub fn apply_axis(&self, delta: i32) -> Result<i32, PointerAccelError> {
        let delta_i64 = i64::from(delta);
        let sign = delta_i64.signum();
        let magnitude = delta_i64.abs();
        let threshold = i64::from(self.config.threshold());
        if magnitude <= threshold {
            return Ok(delta);
        }

        let scaled = threshold
            + ((magnitude - threshold) * i64::from(self.config.numerator()))
                / i64::from(self.config.denominator());
        let bounded = scaled.min(i64::from(self.config.max_output()));
        Ok((bounded * sign) as i32)
    }
}
