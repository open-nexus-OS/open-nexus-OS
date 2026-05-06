// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: `hidrawd` behavior-first host tests for bounded keyboard/mouse ingest.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Stable
//! TEST_COVERAGE: registration, deterministic ingest, and reject taxonomy
//! ADR: docs/adr/0029-input-v1-host-core-architecture.md

use hid::{AbsoluteAxis, HidEvent, HidEventKind, TimestampNs};
use hidrawd::{
    normalize_ingress_batch, DeviceId, HidDeviceKind, HidrawdError, HidrawdService, IngressRole,
    RawIngressBatch, RawIngressEvent, RawIngressEventKind,
};

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

#[test]
fn ingest_device_events_accepts_absolute_pointer_batches() {
    let mut service = HidrawdService::new();
    let mouse_id = DeviceId::new(9);
    service.register_mouse(mouse_id);

    let batch = service
        .ingest_device_events(
            mouse_id,
            HidDeviceKind::Mouse,
            vec![
                HidEvent::abs(TimestampNs::new(4), AbsoluteAxis::X.event_code(), 30),
                HidEvent::abs(TimestampNs::new(4), AbsoluteAxis::Y.event_code(), 14),
            ],
        )
        .expect("absolute pointer batch");

    assert_eq!(batch.kind(), HidDeviceKind::Mouse);
    assert_eq!(batch.events().len(), 2);
    assert!(batch.events().iter().all(|event| event.kind() == HidEventKind::Abs));
}

#[test]
fn virtio_raw_pointer_batch_is_normalized_through_explicit_adapter_gate() {
    let mut service = HidrawdService::new();
    let mouse_id = DeviceId::new(10);
    service.register_mouse(mouse_id);

    let outcome = normalize_ingress_batch(
        &mut service,
        mouse_id,
        &RawIngressBatch::new(
            IngressRole::RelativePointer,
            vec![
                RawIngressEvent::new(RawIngressEventKind::Key, 0x110, 1),
                RawIngressEvent::new(RawIngressEventKind::Relative, 0, 3),
                RawIngressEvent::new(RawIngressEventKind::Relative, 1, -2),
            ],
        ),
        TimestampNs::new(20),
        0,
        0,
    )
    .expect("adapter outcome");

    assert_eq!(outcome.evidence().raw_event_count(), 3);
    assert_eq!(outcome.evidence().normalized_event_count(), 3);
    let hid_batch = outcome.hid_batch().expect("normalized hid batch");
    assert_eq!(hid_batch.kind(), HidDeviceKind::Mouse);
    assert_eq!(hid_batch.events().len(), 3);
    assert_eq!(hid_batch.events()[0].kind(), HidEventKind::Btn);
    assert_eq!(hid_batch.events()[1].kind(), HidEventKind::Rel);
    assert_eq!(hid_batch.events()[2].value().raw(), -2);
    let wire_batch = outcome.wire_batch().expect("wire batch");
    assert_eq!(wire_batch.raw_event_count, 3);
    assert_eq!(wire_batch.normalized_event_count, 3);
}

#[test]
fn test_reject_keyboard_role_cannot_normalize_pointer_motion() {
    let mut service = HidrawdService::new();
    let keyboard_id = DeviceId::new(11);
    service.register_keyboard(keyboard_id);

    let outcome = normalize_ingress_batch(
        &mut service,
        keyboard_id,
        &RawIngressBatch::new(
            IngressRole::Keyboard,
            vec![RawIngressEvent::new(RawIngressEventKind::Relative, 0, 5)],
        ),
        TimestampNs::new(21),
        0,
        0,
    )
    .expect("adapter outcome");

    assert_eq!(outcome.evidence().raw_event_count(), 1);
    assert_eq!(outcome.evidence().normalized_event_count(), 0);
    assert!(outcome.hid_batch().is_none());
    assert!(outcome.wire_batch().is_none());
}
