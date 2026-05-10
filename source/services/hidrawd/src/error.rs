// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Stable `hidrawd` reject taxonomy for bounded HID ingest.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `cargo test -p hidrawd -- --nocapture`
//! ADR: docs/adr/0029-input-v1-host-core-architecture.md

use crate::HidDeviceKind;
use core::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HidrawdError {
    KeyboardUnavailable,
    MouseUnavailable,
    UnexpectedDevice {
        expected: HidDeviceKind,
        actual: HidDeviceKind,
    },
    Parse(hid::HidError),
}

impl HidrawdError {
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::KeyboardUnavailable => "hidrawd.device.keyboard_unavailable",
            Self::MouseUnavailable => "hidrawd.device.mouse_unavailable",
            Self::UnexpectedDevice {
                expected: HidDeviceKind::Keyboard,
                ..
            } => "hidrawd.device.expected_keyboard",
            Self::UnexpectedDevice {
                expected: HidDeviceKind::Mouse,
                ..
            } => "hidrawd.device.expected_mouse",
            Self::Parse(err) => err.code(),
        }
    }
}

impl From<hid::HidError> for HidrawdError {
    fn from(value: hid::HidError) -> Self {
        Self::Parse(value)
    }
}

impl fmt::Display for HidrawdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::KeyboardUnavailable => f.write_str("keyboard device not registered"),
            Self::MouseUnavailable => f.write_str("mouse device not registered"),
            Self::UnexpectedDevice { expected, actual } => {
                write!(
                    f,
                    "unexpected HID device kind: expected {expected:?}, got {actual:?}"
                )
            }
            Self::Parse(err) => err.fmt(f),
        }
    }
}

#[cfg(not(all(nexus_env = "os", target_os = "none")))]
impl std::error::Error for HidrawdError {}
