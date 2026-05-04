// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Stable reject taxonomy for repeat configuration and scheduler state.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: No direct tests (covered by 4 integration tests in `tests/input_v1_0_host/tests/repeat_contract.rs`).
//! ADR: docs/adr/0029-input-v1-host-core-architecture.md

use core::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepeatError {
    InvalidDelay,
    InvalidRate,
    InvalidKey,
    NonMonotonicTime,
    TickBudgetExceeded,
}

impl RepeatError {
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::InvalidDelay => "repeat.delay.invalid",
            Self::InvalidRate => "repeat.rate.invalid",
            Self::InvalidKey => "repeat.key.invalid",
            Self::NonMonotonicTime => "repeat.time.non_monotonic",
            Self::TickBudgetExceeded => "repeat.tick.budget_exceeded",
        }
    }
}

impl fmt::Display for RepeatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidDelay => f.write_str("repeat delay must be within the supported range"),
            Self::InvalidRate => f.write_str("repeat rate must be within the supported range"),
            Self::InvalidKey => f.write_str("repeat key must be non-zero"),
            Self::NonMonotonicTime => f.write_str("repeat tick time must be monotonic"),
            Self::TickBudgetExceeded => f.write_str("repeat tick exceeded bounded output budget"),
        }
    }
}

impl std::error::Error for RepeatError {}
