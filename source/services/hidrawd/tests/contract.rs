// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: `hidrawd` behavior-first host tests for bounded keyboard/mouse ingest.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Stable
//! TEST_COVERAGE: registration, deterministic ingest, and reject taxonomy
//! ADR: docs/adr/0029-input-v1-host-core-architecture.md

use hid::{HidEventKind, TimestampNs};
use hidrawd::{DeviceId, HidDeviceKind, HidrawdError, HidrawdService};

#[test]
fn keyboard_and_mouse_ingest_batches_are_deterministic() {
    let mut service = HidrawdService::new();
    let keyboard_id = DeviceId::new(7);
    let mouse_id = DeviceId::new(8);
    service.register_keyboard(keyboard_id);
    service.register_mouse(mouse_id);

    let keyboard = service
        .ingest_keyboard_report(keyboard_id, TimestampNs::new(10), &[0, 0, 0x04, 0, 0, 0, 0, 0])
        .expect("keyboard report");
    let mouse = service
        .ingest_mouse_report(mouse_id, TimestampNs::new(11), &[0b001, 3u8, (-2i8) as u8])
        .expect("mouse report");

    assert!(service.keyboard_ready());
    assert!(service.mouse_ready());
    assert_eq!(keyboard.kind(), HidDeviceKind::Keyboard);
    assert_eq!(keyboard.events().len(), 1);
    assert_eq!(keyboard.events()[0].kind(), HidEventKind::Key);
    assert_eq!(keyboard.events()[0].code().raw(), 0x04);
    assert_eq!(mouse.kind(), HidDeviceKind::Mouse);
    assert_eq!(mouse.events().len(), 3);
    assert_eq!(mouse.events()[0].kind(), HidEventKind::Btn);
    assert_eq!(mouse.events()[1].kind(), HidEventKind::Rel);
    assert_eq!(mouse.events()[1].value().raw(), 3);
    assert_eq!(mouse.events()[2].value().raw(), -2);
    assert_eq!(service.recent_batches().len(), 2);
}

#[test]
fn test_reject_truncated_keyboard_report() {
    let mut service = HidrawdService::new();
    let keyboard_id = DeviceId::new(1);
    service.register_keyboard(keyboard_id);

    let err = service
        .ingest_keyboard_report(keyboard_id, TimestampNs::new(1), &[0, 0, 0x04])
        .expect_err("must reject truncated keyboard report");
    assert_eq!(err.code(), "hid.keyboard.length");
    assert_eq!(
        err,
        HidrawdError::Parse(hid::HidError::InvalidKeyboardReportLength { actual: 3 })
    );
}

#[test]
fn test_reject_duplicate_keyboard_usage() {
    let mut service = HidrawdService::new();
    let keyboard_id = DeviceId::new(1);
    service.register_keyboard(keyboard_id);

    let err = service
        .ingest_keyboard_report(
            keyboard_id,
            TimestampNs::new(2),
            &[0, 0, 0x04, 0x04, 0, 0, 0, 0],
        )
        .expect_err("must reject duplicate keyboard usage");
    assert_eq!(err.code(), "hid.keyboard.duplicate_usage");
}

#[test]
fn test_reject_mouse_buttons_out_of_range() {
    let mut service = HidrawdService::new();
    let mouse_id = DeviceId::new(2);
    service.register_mouse(mouse_id);

    let err = service
        .ingest_mouse_report(mouse_id, TimestampNs::new(3), &[0b1000, 0, 0])
        .expect_err("must reject out-of-range button bits");
    assert_eq!(err.code(), "hid.mouse.button_bits");
}
