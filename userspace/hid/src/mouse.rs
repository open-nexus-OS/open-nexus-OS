// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: USB-HID boot mouse parser for bounded relative motion, wheel and button changes.
//! The boot report is three bytes (buttons, dx, dy); a device may append bytes, and the
//! fourth is the wheel by convention — the measured receiver sends exactly four in the boot
//! protocol (2026-10-04, `docs/board/measurements/2026-10-04-usb-boot-protocol/`). A frame
//! longer than eight bytes is not the boot format: right after the protocol switch the same
//! receiver still sent one report-protocol frame (nine bytes, report id first) — parsed as a
//! boot report it would read as a right click and a scroll, so it is refused.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: integration tests in `tests/input_v1_0_host/tests/hid_contract.rs` (the
//!                measured reports as goldens) and `source/services/hidrawd/tests/contract.rs`.
//! ADR: docs/adr/0029-input-v1-host-core-architecture.md

use crate::{HidError, HidEvent, MouseButton, RelativeAxis, TimestampNs};
use alloc::vec::Vec;

/// Buttons, dx, dy.
const MOUSE_REPORT_MIN: usize = 3;
/// The boot report plus appended bytes; longer is another format.
const MOUSE_REPORT_MAX: usize = 8;
const WHEEL_AT: usize = 3;

#[derive(Debug, Default, Clone)]
pub struct BootMouseParser {
    previous_buttons: u8,
}

impl BootMouseParser {
    #[must_use]
    pub const fn new() -> Self {
        Self { previous_buttons: 0 }
    }

    pub fn parse_report(
        &mut self,
        timestamp: TimestampNs,
        report: &[u8],
    ) -> Result<Vec<HidEvent>, HidError> {
        if !(MOUSE_REPORT_MIN..=MOUSE_REPORT_MAX).contains(&report.len()) {
            return Err(HidError::InvalidMouseReportLength { actual: report.len() });
        }

        let mut events = Vec::new();
        for button in MouseButton::ALL {
            let was_pressed = self.previous_buttons & button.mask() != 0;
            let is_pressed = report[0] & button.mask() != 0;
            if was_pressed != is_pressed {
                events.push(HidEvent::btn(timestamp, button.event_code(), i32::from(is_pressed)));
            }
        }

        let dx = i32::from(i8::from_ne_bytes([report[1]]));
        let dy = i32::from(i8::from_ne_bytes([report[2]]));
        if dx != 0 {
            events.push(HidEvent::rel(timestamp, RelativeAxis::X.event_code(), dx));
        }
        if dy != 0 {
            events.push(HidEvent::rel(timestamp, RelativeAxis::Y.event_code(), dy));
        }
        if let Some(&wheel) = report.get(WHEEL_AT) {
            let wheel = i32::from(i8::from_ne_bytes([wheel]));
            if wheel != 0 {
                events.push(HidEvent::rel(timestamp, RelativeAxis::Wheel.event_code(), wheel));
            }
        }

        self.previous_buttons = report[0];
        Ok(events)
    }
}
