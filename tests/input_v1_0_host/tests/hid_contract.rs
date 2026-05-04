// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Integration tests for USB-HID boot keyboard/mouse parsing behavior.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: 5 integration tests.
//!
//! TEST_SCOPE:
//!   - keyboard press/release and modifier deltas
//!   - mouse relative/button parsing
//!   - malformed keyboard/mouse reject behavior
//!
//! TEST_SCENARIOS:
//!   - keyboard_press_release_and_modifier_transitions_are_deterministic()
//!   - mouse_relative_and_button_reports_are_deterministic()
//!   - test_reject_* keyboard and mouse rejects
//!
//! DEPENDENCIES:
//!   - `hid` crate boot keyboard/mouse parsers
//!
//! ADR: docs/adr/0029-input-v1-host-core-architecture.md

use hid::{
    BootKeyboardParser, BootMouseParser, HidEvent, HidEventKind, KeyboardUsage, MouseButton,
    RelativeAxis, TimestampNs,
};

fn ts(ns: u64) -> TimestampNs {
    TimestampNs::new(ns)
}

fn logical(events: &[HidEvent]) -> Vec<(HidEventKind, u16, i32)> {
    events.iter().map(|event| (event.kind(), event.code().raw(), event.value().raw())).collect()
}

#[test]
fn keyboard_press_release_and_modifier_transitions_are_deterministic() {
    let mut parser = BootKeyboardParser::new();

    let first = parser
        .parse_report(ts(10), &[0x02, 0x00, KeyboardUsage::A.raw(), 0, 0, 0, 0, 0])
        .expect("first report");
    assert_eq!(
        logical(&first),
        vec![
            (HidEventKind::Key, KeyboardUsage::LEFT_SHIFT.event_code(), 1),
            (HidEventKind::Key, KeyboardUsage::A.event_code(), 1),
        ]
    );

    let second =
        parser.parse_report(ts(20), &[0x00, 0x00, 0, 0, 0, 0, 0, 0]).expect("second report");
    assert_eq!(
        logical(&second),
        vec![
            (HidEventKind::Key, KeyboardUsage::A.event_code(), 0),
            (HidEventKind::Key, KeyboardUsage::LEFT_SHIFT.event_code(), 0),
        ]
    );
}

#[test]
fn mouse_relative_and_button_reports_are_deterministic() {
    let mut parser = BootMouseParser::new();

    let pressed = parser.parse_report(ts(30), &[0b001, 5u8, 252u8]).expect("mouse down");
    assert_eq!(
        logical(&pressed),
        vec![
            (HidEventKind::Btn, MouseButton::Left.event_code(), 1),
            (HidEventKind::Rel, RelativeAxis::X.event_code(), 5),
            (HidEventKind::Rel, RelativeAxis::Y.event_code(), -4),
        ]
    );

    let released = parser.parse_report(ts(40), &[0b000, 0, 0]).expect("mouse up");
    assert_eq!(logical(&released), vec![(HidEventKind::Btn, MouseButton::Left.event_code(), 0)]);
}

#[test]
fn test_reject_keyboard_truncated_report() {
    let mut parser = BootKeyboardParser::new();
    let err = parser.parse_report(ts(50), &[0x00, 0x00, KeyboardUsage::A.raw()]).unwrap_err();
    assert_eq!(err.code(), "hid.keyboard.length");
}

#[test]
fn test_reject_keyboard_duplicate_usage() {
    let mut parser = BootKeyboardParser::new();
    let err = parser
        .parse_report(
            ts(60),
            &[0x00, 0x00, KeyboardUsage::A.raw(), KeyboardUsage::A.raw(), 0, 0, 0, 0],
        )
        .unwrap_err();
    assert_eq!(err.code(), "hid.keyboard.duplicate_usage");
}

#[test]
fn test_reject_mouse_invalid_button_bits() {
    let mut parser = BootMouseParser::new();
    let err = parser.parse_report(ts(70), &[0b1000, 0, 0]).unwrap_err();
    assert_eq!(err.code(), "hid.mouse.button_bits");
}
