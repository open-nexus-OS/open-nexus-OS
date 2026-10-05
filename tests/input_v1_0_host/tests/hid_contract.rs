// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Integration tests for USB-HID boot keyboard/mouse parsing behavior.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: 13 integration tests.
//!
//! TEST_SCOPE:
//!   - keyboard press/release and modifier deltas, the reserved byte, the error usages
//!   - mouse relative/button/wheel parsing, the boot reports a measured receiver sends
//!   - the allocation-free form (events appended to a caller's buffer) and release-all
//!   - malformed keyboard/mouse reject behavior
//!
//! TEST_SCENARIOS:
//!   - keyboard_press_release_and_modifier_transitions_are_deterministic()
//!   - keyboard_reserved_byte_is_the_devices_own()
//!   - keyboard_error_usages_keep_the_keys_and_move_the_modifiers()
//!   - mouse_relative_and_button_reports_are_deterministic()
//!   - mouse_boot_reports_as_measured_on_the_board()
//!   - reports_append_into_one_reused_buffer()
//!   - release_all_lets_go_of_every_held_key_and_button()
//!   - test_reject_* keyboard and mouse rejects (a refused report leaves the buffer untouched)
//!
//! DEPENDENCIES:
//!   - `hid` crate boot keyboard/mouse parsers
//!
//! ADR: docs/adr/0029-input-v1-host-core-architecture.md

use hid::{
    AbsoluteAxis, BootKeyboardParser, BootMouseParser, HidEvent, HidEventKind, KeyboardUsage,
    MouseButton, RelativeAxis, TimestampNs,
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
fn test_reject_keyboard_overlong_report() {
    let mut parser = BootKeyboardParser::new();
    let err = parser.parse_report(ts(52), &[0; 9]).unwrap_err();
    assert_eq!(err.code(), "hid.keyboard.length");
}

#[test]
fn keyboard_reserved_byte_is_the_devices_own() {
    let mut parser = BootKeyboardParser::new();
    let events = parser
        .parse_report(ts(54), &[0x00, 0x5a, KeyboardUsage::A.raw(), 0, 0, 0, 0, 0])
        .expect("a non-zero reserved byte is not an error");
    assert_eq!(logical(&events), vec![(HidEventKind::Key, KeyboardUsage::A.event_code(), 1)]);
}

#[test]
fn keyboard_error_usages_keep_the_keys_and_move_the_modifiers() {
    let mut parser = BootKeyboardParser::new();
    parser.parse_report(ts(56), &[0x00, 0x00, KeyboardUsage::A.raw(), 0, 0, 0, 0, 0]).expect("A");
    // ErrorRollOver in every slot, left shift pressed meanwhile: A stays down, shift moves.
    let rollover = parser.parse_report(ts(57), &[0x02, 0x00, 1, 1, 1, 1, 1, 1]).expect("rollover");
    assert_eq!(
        logical(&rollover),
        vec![(HidEventKind::Key, KeyboardUsage::LEFT_SHIFT.event_code(), 1)]
    );
    for usage in [0x02u8, 0x03] {
        let error = parser.parse_report(ts(58), &[0x02, 0x00, usage, 0, 0, 0, 0, 0]);
        assert_eq!(error.map(|e| e.len()), Ok(0), "usage {usage:#04x} keeps the state");
    }
    let released = parser.parse_report(ts(59), &[0x00, 0x00, 0, 0, 0, 0, 0, 0]).expect("up");
    assert_eq!(
        logical(&released),
        vec![
            (HidEventKind::Key, KeyboardUsage::A.event_code(), 0),
            (HidEventKind::Key, KeyboardUsage::LEFT_SHIFT.event_code(), 0),
        ]
    );
}

/// Reports a radio receiver's boot-mouse interface sent in the boot protocol (2026-10-04,
/// `docs/board/measurements/2026-10-04-usb-boot-protocol/capture-mouse.txt`): four bytes,
/// the thumb buttons on bits 3 and 4, the wheel in byte 3.
#[test]
fn mouse_boot_reports_as_measured_on_the_board() {
    let mut parser = BootMouseParser::new();
    let moved = parser.parse_report(ts(70), &[0x00, 0xaa, 0x7f, 0x00]).expect("motion");
    assert_eq!(
        logical(&moved),
        vec![
            (HidEventKind::Rel, RelativeAxis::X.event_code(), -86),
            (HidEventKind::Rel, RelativeAxis::Y.event_code(), 127),
        ]
    );
    let side = parser.parse_report(ts(71), &[0x08, 0x00, 0x00, 0x00]).expect("side");
    assert_eq!(logical(&side), vec![(HidEventKind::Btn, MouseButton::Side.event_code(), 1)]);
    let extra = parser.parse_report(ts(72), &[0x10, 0x00, 0x00, 0x00]).expect("extra");
    assert_eq!(
        logical(&extra),
        vec![
            (HidEventKind::Btn, MouseButton::Side.event_code(), 0),
            (HidEventKind::Btn, MouseButton::Extra.event_code(), 1),
        ]
    );
    let down = parser.parse_report(ts(73), &[0x00, 0x00, 0x00, 0xff]).expect("wheel down");
    assert_eq!(
        logical(&down),
        vec![
            (HidEventKind::Btn, MouseButton::Extra.event_code(), 0),
            (HidEventKind::Rel, RelativeAxis::Wheel.event_code(), -1),
        ]
    );
    let up = parser.parse_report(ts(74), &[0x00, 0x00, 0x00, 0x01]).expect("wheel up");
    assert_eq!(logical(&up), vec![(HidEventKind::Rel, RelativeAxis::Wheel.event_code(), 1)]);
    assert_eq!(MouseButton::Side.event_code(), 0x113);
    assert_eq!(MouseButton::Task.event_code(), 0x117);
}

/// The same receiver's first frame after the switch to the boot protocol was still in the
/// report format (nine bytes, report id 2 first): as a boot report it would read as a right
/// click and a scroll. Refused by its length; so is a frame too short to be a boot report.
#[test]
fn test_reject_mouse_report_protocol_frame_after_the_switch() {
    let mut parser = BootMouseParser::new();
    let stale = [0x02, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x00, 0x00];
    assert_eq!(parser.parse_report(ts(75), &stale).unwrap_err().code(), "hid.mouse.length");
    assert_eq!(parser.parse_report(ts(76), &[0x01, 0x02]).unwrap_err().code(), "hid.mouse.length");
    // Nothing leaked into the state: the next real report presses nothing that was not pressed.
    let next = parser.parse_report(ts(77), &[0x00, 0x01, 0x00, 0x00]).expect("motion");
    assert_eq!(logical(&next), vec![(HidEventKind::Rel, RelativeAxis::X.event_code(), 1)]);
}

/// The live ingress keeps ONE buffer for every report (a mouse reports every millisecond): the
/// events of each report are appended after what is already there.
#[test]
fn reports_append_into_one_reused_buffer() {
    let mut keyboard = BootKeyboardParser::new();
    let mut mouse = BootMouseParser::new();
    let mut out = Vec::with_capacity(16);
    keyboard
        .parse_report_into(ts(90), &[0, 0, KeyboardUsage::A.raw(), 0, 0, 0, 0, 0], &mut out)
        .expect("A");
    mouse.parse_report_into(ts(91), &[0x01, 0x02, 0x00, 0x00], &mut out).expect("click + motion");
    assert_eq!(
        logical(&out),
        vec![
            (HidEventKind::Key, KeyboardUsage::A.raw() as u16, 1),
            (HidEventKind::Btn, MouseButton::Left.event_code(), 1),
            (HidEventKind::Rel, RelativeAxis::X.event_code(), 2),
        ]
    );
    let capacity = out.capacity();
    out.clear();
    mouse.parse_report_into(ts(92), &[0x01, 0x00, 0x03, 0x00], &mut out).expect("motion");
    assert_eq!(out.capacity(), capacity, "no allocation on the second report");
}

/// A device that went away leaves nothing pressed: every held key, modifier and button is
/// released once, and the parser starts from nothing.
#[test]
fn release_all_lets_go_of_every_held_key_and_button() {
    let mut keyboard = BootKeyboardParser::new();
    let mut out = Vec::new();
    let held = [0x02, 0, KeyboardUsage::A.raw(), 0x05, 0, 0, 0, 0];
    keyboard.parse_report_into(ts(93), &held, &mut out).expect("shift + a + b");
    out.clear();
    keyboard.release_all_into(ts(94), &mut out);
    assert_eq!(
        logical(&out),
        vec![
            (HidEventKind::Key, KeyboardUsage::A.raw() as u16, 0),
            (HidEventKind::Key, 0x05, 0),
            (HidEventKind::Key, KeyboardUsage::modifier_from_bit(1).event_code(), 0),
        ]
    );
    out.clear();
    keyboard.release_all_into(ts(95), &mut out);
    assert!(out.is_empty(), "released once");

    let mut mouse = BootMouseParser::new();
    mouse.parse_report_into(ts(96), &[0b101, 0, 0, 0], &mut out).expect("left + middle");
    out.clear();
    mouse.release_all_into(ts(97), &mut out);
    assert_eq!(
        logical(&out),
        vec![
            (HidEventKind::Btn, MouseButton::Left.event_code(), 0),
            (HidEventKind::Btn, MouseButton::Middle.event_code(), 0),
        ]
    );
}

/// A refused report appends nothing: the buffer keeps the events of the reports before it.
#[test]
fn test_reject_refused_report_leaves_the_buffer_untouched() {
    let mut keyboard = BootKeyboardParser::new();
    let mut mouse = BootMouseParser::new();
    let mut out = Vec::new();
    mouse.parse_report_into(ts(98), &[0x00, 0x01, 0x00], &mut out).expect("motion");
    let before = logical(&out);
    assert!(mouse.parse_report_into(ts(99), &[0; 9], &mut out).is_err());
    let duplicate = [0, 0, KeyboardUsage::A.raw(), KeyboardUsage::A.raw(), 0, 0, 0, 0];
    assert!(keyboard.parse_report_into(ts(99), &duplicate, &mut out).is_err());
    assert!(keyboard.parse_report_into(ts(99), &[0; 7], &mut out).is_err());
    assert_eq!(logical(&out), before);
}

#[test]
fn absolute_pointer_events_are_typed_and_distinct_from_relative_axes() {
    let events = [
        HidEvent::abs(ts(80), AbsoluteAxis::X.event_code(), 320),
        HidEvent::abs(ts(80), AbsoluteAxis::Y.event_code(), 200),
    ];

    assert_eq!(
        logical(&events),
        vec![
            (HidEventKind::Abs, AbsoluteAxis::X.event_code(), 320),
            (HidEventKind::Abs, AbsoluteAxis::Y.event_code(), 200),
        ]
    );
}
