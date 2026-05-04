// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: USB-HID boot keyboard parser with deterministic delta emission.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: No direct tests (covered by 5 integration tests in `tests/input_v1_0_host/tests/hid_contract.rs`).
//! ADR: docs/adr/0029-input-v1-host-core-architecture.md

use crate::{HidError, HidEvent, KeyboardUsage, TimestampNs};

const KEYBOARD_REPORT_LEN: usize = 8;
const MAX_KEYS: usize = 6;

#[derive(Debug, Default, Clone)]
pub struct BootKeyboardParser {
    previous_modifiers: u8,
    previous_keys: [u8; MAX_KEYS],
}

impl BootKeyboardParser {
    #[must_use]
    pub const fn new() -> Self {
        Self { previous_modifiers: 0, previous_keys: [0; MAX_KEYS] }
    }

    pub fn parse_report(
        &mut self,
        timestamp: TimestampNs,
        report: &[u8],
    ) -> Result<Vec<HidEvent>, HidError> {
        if report.len() != KEYBOARD_REPORT_LEN {
            return Err(HidError::InvalidKeyboardReportLength { actual: report.len() });
        }
        if report[1] != 0 {
            return Err(HidError::KeyboardReservedByteNonZero { value: report[1] });
        }

        let mut current_keys = [0u8; MAX_KEYS];
        current_keys.copy_from_slice(&report[2..]);
        validate_no_duplicates(&current_keys)?;

        let mut events = Vec::new();
        for released in key_deltas(&self.previous_keys, &current_keys) {
            events.push(HidEvent::key(timestamp, released as u16, 0));
        }
        for bit in 0..8 {
            let mask = 1u8 << bit;
            if self.previous_modifiers & mask != 0 && report[0] & mask == 0 {
                events.push(HidEvent::key(
                    timestamp,
                    KeyboardUsage::modifier_from_bit(bit).event_code(),
                    0,
                ));
            }
        }
        for bit in 0..8 {
            let mask = 1u8 << bit;
            if self.previous_modifiers & mask == 0 && report[0] & mask != 0 {
                events.push(HidEvent::key(
                    timestamp,
                    KeyboardUsage::modifier_from_bit(bit).event_code(),
                    1,
                ));
            }
        }
        for pressed in key_deltas(&current_keys, &self.previous_keys) {
            events.push(HidEvent::key(timestamp, pressed as u16, 1));
        }

        self.previous_modifiers = report[0];
        self.previous_keys = current_keys;
        Ok(events)
    }
}

fn validate_no_duplicates(keys: &[u8; MAX_KEYS]) -> Result<(), HidError> {
    for (idx, usage) in keys.iter().enumerate() {
        if *usage == 0 {
            continue;
        }
        if keys.iter().skip(idx + 1).any(|candidate| candidate == usage) {
            return Err(HidError::DuplicateKeyUsage { usage: *usage });
        }
    }
    Ok(())
}

fn key_deltas(lhs: &[u8; MAX_KEYS], rhs: &[u8; MAX_KEYS]) -> Vec<u8> {
    let mut out = Vec::new();
    for usage in lhs.iter().copied().filter(|usage| *usage != 0) {
        if !rhs.contains(&usage) {
            out.push(usage);
        }
    }
    out.sort_unstable();
    out
}
