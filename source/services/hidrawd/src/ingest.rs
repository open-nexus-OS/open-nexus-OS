// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Thin ingest adapters over the RFC-0052 HID parser authority.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `cargo test -p hidrawd -- --nocapture`
//! ADR: docs/adr/0029-input-v1-host-core-architecture.md

use crate::HidrawdError;
use alloc::vec::Vec;
use hid::{BootKeyboardParser, BootMouseParser, HidEvent, TimestampNs};

extern crate alloc;

pub(crate) fn parse_keyboard(
    parser: &mut BootKeyboardParser,
    timestamp: TimestampNs,
    report: &[u8],
) -> Result<Vec<HidEvent>, HidrawdError> {
    parser.parse_report(timestamp, report).map_err(HidrawdError::from)
}

pub(crate) fn parse_mouse(
    parser: &mut BootMouseParser,
    timestamp: TimestampNs,
    report: &[u8],
) -> Result<Vec<HidEvent>, HidrawdError> {
    parser.parse_report(timestamp, report).map_err(HidrawdError::from)
}
