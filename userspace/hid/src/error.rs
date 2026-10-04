// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Stable reject taxonomy for USB-HID boot parser failures.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: the `test_reject_*` integration tests in `tests/input_v1_0_host/tests/hid_contract.rs`
//!                and `source/services/hidrawd/tests/contract.rs`.
//! ADR: docs/adr/0029-input-v1-host-core-architecture.md

use core::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HidError {
    InvalidKeyboardReportLength { actual: usize },
    InvalidMouseReportLength { actual: usize },
    DuplicateKeyUsage { usage: u8 },
}

impl HidError {
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::InvalidKeyboardReportLength { .. } => "hid.keyboard.length",
            Self::InvalidMouseReportLength { .. } => "hid.mouse.length",
            Self::DuplicateKeyUsage { .. } => "hid.keyboard.duplicate_usage",
        }
    }
}

impl fmt::Display for HidError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidKeyboardReportLength { actual } => {
                write!(f, "invalid keyboard report length: {actual}")
            }
            Self::InvalidMouseReportLength { actual } => {
                write!(f, "invalid mouse report length: {actual}")
            }
            Self::DuplicateKeyUsage { usage } => {
                write!(f, "duplicate key usage in report: {usage:#04x}")
            }
        }
    }
}

#[cfg(not(all(nexus_env = "os", target_os = "none")))]
impl std::error::Error for HidError {}
