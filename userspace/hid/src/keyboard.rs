// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: USB-HID boot keyboard parser with deterministic delta emission. The report is eight
//! bytes: modifiers, a reserved byte (the device's to use — ignored), six key slots. When the
//! keyboard cannot tell which keys are down (more pressed than it can report, or an internal
//! error) it fills the slots with the error usages 0x01..=0x03 (ErrorRollOver, POSTFail,
//! ErrorUndefined): the keys stay as they were, only the modifiers move. A measured keyboard
//! drops the extra keys instead (2026-10-04, `docs/board/measurements/
//! 2026-10-04-usb-boot-protocol/`); both behaviours are the boot protocol's.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: integration tests in `tests/input_v1_0_host/tests/hid_contract.rs` and
//!                `source/services/hidrawd/tests/contract.rs`.
//! ADR: docs/adr/0029-input-v1-host-core-architecture.md

use crate::{HidError, HidEvent, KeyboardUsage, TimestampNs};
use alloc::vec::Vec;

const KEYBOARD_REPORT_LEN: usize = 8;
const MAX_KEYS: usize = 6;
/// ErrorRollOver, POSTFail, ErrorUndefined: the key state is unknown.
const ERROR_USAGES: core::ops::RangeInclusive<u8> = 0x01..=0x03;

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

    /// Appends the report's events to `out`: releases, modifier changes, presses. Nothing is
    /// allocated once `out` holds a report's worth (the live ingress reuses one buffer for
    /// every report). A refused report leaves `out` and the key state untouched.
    pub fn parse_report_into(
        &mut self,
        timestamp: TimestampNs,
        report: &[u8],
        out: &mut Vec<HidEvent>,
    ) -> Result<(), HidError> {
        if report.len() != KEYBOARD_REPORT_LEN {
            return Err(HidError::InvalidKeyboardReportLength { actual: report.len() });
        }

        let mut current_keys = [0u8; MAX_KEYS];
        current_keys.copy_from_slice(&report[2..]);
        if current_keys.iter().any(|usage| ERROR_USAGES.contains(usage)) {
            current_keys = self.previous_keys;
        }
        validate_no_duplicates(&current_keys)?;
        self.advance(timestamp, report[0], current_keys, out);
        Ok(())
    }

    /// Releases everything held — keys, then modifiers, as key-up events — and forgets it: a
    /// keyboard that went away (unplugged, its pipe dropped) leaves nothing pressed.
    pub fn release_all_into(&mut self, timestamp: TimestampNs, out: &mut Vec<HidEvent>) {
        self.advance(timestamp, 0, [0; MAX_KEYS], out);
    }

    /// Moves from the held state to (`modifiers`, `keys`), appending the difference.
    fn advance(
        &mut self,
        timestamp: TimestampNs,
        modifiers: u8,
        keys: [u8; MAX_KEYS],
        out: &mut Vec<HidEvent>,
    ) {
        let (released, n) = key_deltas(&self.previous_keys, &keys);
        for &usage in &released[..n] {
            out.push(HidEvent::key(timestamp, u16::from(usage), 0));
        }
        for bit in 0..8 {
            let mask = 1u8 << bit;
            if self.previous_modifiers & mask != 0 && modifiers & mask == 0 {
                out.push(HidEvent::key(
                    timestamp,
                    KeyboardUsage::modifier_from_bit(bit).event_code(),
                    0,
                ));
            }
        }
        for bit in 0..8 {
            let mask = 1u8 << bit;
            if self.previous_modifiers & mask == 0 && modifiers & mask != 0 {
                out.push(HidEvent::key(
                    timestamp,
                    KeyboardUsage::modifier_from_bit(bit).event_code(),
                    1,
                ));
            }
        }
        let (pressed, n) = key_deltas(&keys, &self.previous_keys);
        for &usage in &pressed[..n] {
            out.push(HidEvent::key(timestamp, u16::from(usage), 1));
        }

        self.previous_modifiers = modifiers;
        self.previous_keys = keys;
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

/// The usages in `lhs` that `rhs` lacks, ascending, and how many (at most six: no allocation).
fn key_deltas(lhs: &[u8; MAX_KEYS], rhs: &[u8; MAX_KEYS]) -> ([u8; MAX_KEYS], usize) {
    let mut out = [0u8; MAX_KEYS];
    let mut n = 0;
    for usage in lhs.iter().copied().filter(|usage| *usage != 0) {
        if !rhs.contains(&usage) {
            out[n] = usage;
            n += 1;
        }
    }
    out[..n].sort_unstable();
    (out, n)
}
