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

    /// The report's events as a fresh list — [`Self::parse_report_into`] for a caller without
    /// a buffer of its own.
    pub fn parse_report(
        &mut self,
        timestamp: TimestampNs,
        report: &[u8],
    ) -> Result<Vec<HidEvent>, HidError> {
        let mut events = Vec::new();
        self.parse_report_into(timestamp, report, &mut events)?;
        Ok(events)
    }

    /// Appends the report's events to `out`: button changes, then motion, then the wheel.
    /// Nothing is allocated once `out` holds a report's worth (a mouse reports every
    /// millisecond). A refused report leaves `out` and the button state untouched.
    pub fn parse_report_into(
        &mut self,
        timestamp: TimestampNs,
        report: &[u8],
        out: &mut Vec<HidEvent>,
    ) -> Result<(), HidError> {
        if !(MOUSE_REPORT_MIN..=MOUSE_REPORT_MAX).contains(&report.len()) {
            return Err(HidError::InvalidMouseReportLength { actual: report.len() });
        }

        self.buttons(timestamp, report[0], out);
        let dx = i32::from(i8::from_ne_bytes([report[1]]));
        let dy = i32::from(i8::from_ne_bytes([report[2]]));
        if dx != 0 {
            out.push(HidEvent::rel(timestamp, RelativeAxis::X.event_code(), dx));
        }
        if dy != 0 {
            out.push(HidEvent::rel(timestamp, RelativeAxis::Y.event_code(), dy));
        }
        if let Some(&wheel) = report.get(WHEEL_AT) {
            let wheel = i32::from(i8::from_ne_bytes([wheel]));
            if wheel != 0 {
                out.push(HidEvent::rel(timestamp, RelativeAxis::Wheel.event_code(), wheel));
            }
        }
        Ok(())
    }

    /// Releases every held button and forgets it: a mouse that went away (unplugged, its pipe
    /// dropped) leaves nothing pressed.
    pub fn release_all_into(&mut self, timestamp: TimestampNs, out: &mut Vec<HidEvent>) {
        self.buttons(timestamp, 0, out);
    }

    /// Moves the held buttons to `buttons`, appending the changes.
    fn buttons(&mut self, timestamp: TimestampNs, buttons: u8, out: &mut Vec<HidEvent>) {
        for button in MouseButton::ALL {
            let was_pressed = self.previous_buttons & button.mask() != 0;
            let is_pressed = buttons & button.mask() != 0;
            if was_pressed != is_pressed {
                out.push(HidEvent::btn(timestamp, button.event_code(), i32::from(is_pressed)));
            }
        }
        self.previous_buttons = buttons;
    }
}
