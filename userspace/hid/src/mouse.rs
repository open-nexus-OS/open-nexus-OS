// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: USB-HID boot mouse parser for bounded relative motion and button changes.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: No direct tests (covered by 5 integration tests in `tests/input_v1_0_host/tests/hid_contract.rs`).
//! ADR: docs/adr/0029-input-v1-host-core-architecture.md

use crate::{HidError, HidEvent, MouseButton, RelativeAxis, TimestampNs};

const MOUSE_REPORT_LEN: usize = 3;
const BUTTON_MASK: u8 = 0b111;

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
        if report.len() != MOUSE_REPORT_LEN {
            return Err(HidError::InvalidMouseReportLength { actual: report.len() });
        }
        if report[0] & !BUTTON_MASK != 0 {
            return Err(HidError::MouseButtonsOutOfRange { value: report[0] });
        }

        let mut events = Vec::new();
        for button in [MouseButton::Left, MouseButton::Right, MouseButton::Middle] {
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

        self.previous_buttons = report[0];
        Ok(events)
    }
}
