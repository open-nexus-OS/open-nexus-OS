// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: RFC-0113 §1 on the host — a keyboard batch's stream sequence crosses the wire decode,
//! the key it produced carries it with the time the system first saw it, and an auto-repeat
//! carries its held key's sequence with the time it came due. (Out of `contract.rs` under the
//! module-size ratchet.)
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: the keyboard wire decode without a pointer source; `seq`/`t_ns` on a key and on
//!   its repeat
//! ADR: docs/adr/0090-the-input-source-decides-the-route-windowd-orders-focus-and-keys-the-compositor-owns-the-caret.md

use hid::TimestampNs;
use hidrawd::{DeviceId, HidBatch, HidrawdService};
use input_live_protocol::{
    WireHidBatch, WireHidEvent, EVENT_KIND_KEY, HID_KIND_KEYBOARD, POINTER_SOURCE_NONE,
};
use inputd::{InputDispatch, InputdConfig, InputdService};
use windowd::{CallerCtx, CommitSeq, Layer, Rect, SurfaceBuffer, WindowServer, WindowdConfig};

fn fixture_server() -> WindowServer {
    let caller = CallerCtx::from_service_metadata(0x55);
    let mut server =
        WindowServer::new(WindowdConfig { width: 64, height: 48, hz: 60 }).expect("server");
    let buffer =
        SurfaceBuffer::solid(caller, 50, 24, 16, [0x24, 0x28, 0x34, 0xff]).expect("buffer");
    let surface = server.create_surface(caller, buffer.clone()).expect("surface");
    server.queue_buffer(caller, surface, buffer, &[Rect::new(0, 0, 24, 16)]).expect("queue");
    server
        .commit_scene(
            CallerCtx::system(),
            CommitSeq::new(1),
            &[Layer { surface, x: 8, y: 8, z: 0 }],
        )
        .expect("scene");
    let _ack = server.present_tick().expect("present tick").expect("present");
    server
}

/// inputd with a focused surface (keyboard routing needs one): repeat delay 100 ms, 10 Hz.
fn focused_inputd() -> InputdService<WindowServer> {
    let config = InputdConfig::new("de", 100, 10, 1, 2, 1, 32, 16, 12, 12).expect("config");
    let mut inputd = InputdService::new(fixture_server(), config).expect("inputd");
    let mut hidrawd = HidrawdService::new();
    let mouse_id = DeviceId::new(2);
    hidrawd.register_mouse(mouse_id);
    let click = hidrawd
        .ingest_mouse_report(mouse_id, TimestampNs::new(1), &[0b001, 0, 0])
        .expect("focus click");
    inputd.apply_hid_batch(&click).expect("focus route");
    inputd
}

/// The key `a` pressed at 16 ns as event 41 of keyboard 4's stream, through the wire decode.
fn key_a_as_event_41() -> HidBatch {
    let transform = inputd::visible_pointer_transform().expect("pointer transform");
    inputd::decode_wire_batch(
        WireHidBatch {
            device_kind: HID_KIND_KEYBOARD,
            device_id: 4,
            pointer_source: POINTER_SOURCE_NONE,
            abs_max_x: 0,
            abs_max_y: 0,
            raw_event_count: 1,
            normalized_event_count: 1,
            seq_first: 41,
            events: vec![WireHidEvent {
                kind: EVENT_KIND_KEY,
                code: 0x04,
                value: 1,
                timestamp_ns: 16,
            }],
        },
        transform,
    )
    .expect("keyboard wire batch")
}

#[test]
fn keyboard_wire_batch_decodes_without_pointer_source() {
    let hid_batch = key_a_as_event_41();
    assert_eq!(hid_batch.kind(), hidrawd::HidDeviceKind::Keyboard);
    assert_eq!(hid_batch.pointer_source(), None);
    assert_eq!(hid_batch.events().len(), 1);
    assert_eq!(hid_batch.events()[0].kind(), hid::HidEventKind::Key);
    assert_eq!(hid_batch.seq_first(), 41, "the stream sequence crosses the decode (RFC-0113 §1)");
}

#[test]
fn a_key_carries_its_sequence_and_the_time_it_was_first_seen() {
    let mut inputd = focused_inputd();
    let dispatches = inputd.apply_hid_batch(&key_a_as_event_41()).expect("keyboard route");
    assert!(
        matches!(dispatches.as_slice(), [InputDispatch::Keyboard { seq: 41, t_ns: 16, .. }]),
        "{dispatches:?}"
    );
}

#[test]
fn a_repeat_carries_its_held_keys_sequence_and_the_time_it_came_due() {
    let mut inputd = focused_inputd();
    inputd.apply_hid_batch(&key_a_as_event_41()).expect("keyboard route");
    assert_eq!(inputd.next_repeat_deadline_ns(), Some(16 + 100_000_000), "a timer only while held");
    let repeat = inputd.tick_repeat(100_000_016).expect("the first repeat");
    assert!(
        matches!(
            repeat.as_slice(),
            [InputDispatch::Keyboard { repeated: true, seq: 41, t_ns: 100_000_016, .. }]
        ),
        "{repeat:?}"
    );
}
