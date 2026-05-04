// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Stable `touchd` reject taxonomy for bounded touch ingest and synthetic mode.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `cargo test -p touchd -- --nocapture`
//! ADR: docs/adr/0029-input-v1-host-core-architecture.md

use core::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TouchdError {
    DeviceUnavailable,
    SyntheticModeDisabled,
    Normalize(touch::TouchError),
}

impl TouchdError {
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::DeviceUnavailable => "touchd.device.unavailable",
            Self::SyntheticModeDisabled => "touchd.synthetic.disabled",
            Self::Normalize(err) => err.code(),
        }
    }
}

impl From<touch::TouchError> for TouchdError {
    fn from(value: touch::TouchError) -> Self {
        Self::Normalize(value)
    }
}

impl fmt::Display for TouchdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DeviceUnavailable => f.write_str("touch device not registered"),
            Self::SyntheticModeDisabled => f.write_str("synthetic touch mode is disabled"),
            Self::Normalize(err) => err.fmt(f),
        }
    }
}

#[cfg(not(all(nexus_env = "os", target_os = "none")))]
impl std::error::Error for TouchdError {}
