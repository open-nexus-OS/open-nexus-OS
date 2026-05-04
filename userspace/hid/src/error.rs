// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Stable reject taxonomy for USB-HID boot parser failures.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: No direct tests (covered by 5 integration tests in `tests/input_v1_0_host/tests/hid_contract.rs`).
//! ADR: docs/adr/0029-input-v1-host-core-architecture.md

use core::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HidError {
    InvalidKeyboardReportLength { actual: usize },
    InvalidMouseReportLength { actual: usize },
    KeyboardReservedByteNonZero { value: u8 },
    DuplicateKeyUsage { usage: u8 },
    MouseButtonsOutOfRange { value: u8 },
}

impl HidError {
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::InvalidKeyboardReportLength { .. } => "hid.keyboard.length",
            Self::InvalidMouseReportLength { .. } => "hid.mouse.length",
            Self::KeyboardReservedByteNonZero { .. } => "hid.keyboard.reserved_byte",
            Self::DuplicateKeyUsage { .. } => "hid.keyboard.duplicate_usage",
            Self::MouseButtonsOutOfRange { .. } => "hid.mouse.button_bits",
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
            Self::KeyboardReservedByteNonZero { value } => {
                write!(f, "keyboard reserved byte must be zero: {value}")
            }
            Self::DuplicateKeyUsage { usage } => {
                write!(f, "duplicate key usage in report: {usage:#04x}")
            }
            Self::MouseButtonsOutOfRange { value } => {
                write!(f, "mouse buttons out of boot-protocol range: {value:#04b}")
            }
        }
    }
}

impl std::error::Error for HidError {}
