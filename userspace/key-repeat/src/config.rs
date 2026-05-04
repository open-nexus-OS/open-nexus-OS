// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Repeat configuration and typed validation for bounded host scheduling.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: No direct tests (covered by 4 integration tests in `tests/input_v1_0_host/tests/repeat_contract.rs`).
//! ADR: docs/adr/0029-input-v1-host-core-architecture.md

use crate::RepeatError;

const MAX_DELAY_MS: u32 = 5_000;
const MAX_RATE_HZ: u16 = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DelayMs(u32);

impl DelayMs {
    pub fn new(raw: u32) -> Result<Self, RepeatError> {
        if raw == 0 || raw > MAX_DELAY_MS {
            return Err(RepeatError::InvalidDelay);
        }
        Ok(Self(raw))
    }

    #[must_use]
    pub const fn raw(self) -> u32 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RateHz(u16);

impl RateHz {
    pub fn new(raw: u16) -> Result<Self, RepeatError> {
        if raw == 0 || raw > MAX_RATE_HZ {
            return Err(RepeatError::InvalidRate);
        }
        Ok(Self(raw))
    }

    #[must_use]
    pub const fn raw(self) -> u16 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RepeatKey(u16);

impl RepeatKey {
    pub fn new(raw: u16) -> Result<Self, RepeatError> {
        if raw == 0 {
            return Err(RepeatError::InvalidKey);
        }
        Ok(Self(raw))
    }

    #[must_use]
    pub const fn raw(self) -> u16 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RepeatConfig {
    delay: DelayMs,
    rate: RateHz,
}

impl RepeatConfig {
    pub fn new(delay: DelayMs, rate: RateHz) -> Result<Self, RepeatError> {
        if rate.raw() == 0 {
            return Err(RepeatError::InvalidRate);
        }
        Ok(Self { delay, rate })
    }

    #[must_use]
    pub const fn delay(self) -> DelayMs {
        self.delay
    }

    #[must_use]
    pub const fn rate(self) -> RateHz {
        self.rate
    }

    #[must_use]
    pub const fn period_ns(self) -> u64 {
        1_000_000_000u64 / self.rate.raw() as u64
    }
}
