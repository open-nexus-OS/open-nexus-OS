// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: `inputd` behavior-first host tests for config validation and `windowd` routing.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Stable
//! TEST_COVERAGE: config rejects, bounded queue overflow, stale-route reject, repeat determinism
//! ADR: docs/adr/0029-input-v1-host-core-architecture.md

use hid::{AbsoluteAxis, HidEvent, TimestampNs};
use hidrawd::{DeviceId, HidrawdService};
use inputd::{InputDispatch, InputdConfig, InputdError, InputdService};
use touch::{RawTouchSample, TouchBounds, TouchPhase, TouchTimestampNs};
use touchd::{SyntheticTouchMode, TouchDeviceId, TouchdService};
use windowd::{CallerCtx, CommitSeq, Layer, Rect, SurfaceBuffer, WindowServer, WindowdConfig};

fn fixture_server() -> (WindowServer, CallerCtx, windowd::SurfaceId) {
    let caller = CallerCtx::from_service_metadata(0x55);
    let mut server = WindowServer::new(WindowdConfig { width: 64, height: 48, hz: 60 }).expect("server");
    let buffer = SurfaceBuffer::solid(caller, 50, 24, 16, [0x24, 0x28, 0x34, 0xff]).expect("buffer");
    let surface = server.create_surface(caller, buffer.clone()).expect("surface");
    server
        .queue_buffer(caller, surface, buffer, &[Rect::new(0, 0, 24, 16)])
        .expect("queue");
    server
        .commit_scene(CallerCtx::system(), CommitSeq::new(1), &[Layer { surface, x: 8, y: 8, z: 0 }])
        .expect("scene");
    server.present_tick().expect("present tick").expect("present");
    (server, caller, surface)
}

fn config(queue_capacity: usize) -> InputdConfig {
    InputdConfig::new("de", 100, 10, 1, 2, 1, 32, queue_capacity, 12, 12).expect("config")
}

#[test]
fn test_reject_unknown_layout_name() {
    let err = InputdConfig::new("xx", 100, 10, 1, 2, 1, 32, 8, 12, 12).expect_err("layout reject");
    assert_eq!(err.code(), "keymap.layout.unknown");
}

#[test]
fn test_reject_invalid_repeat_config() {
    let err = InputdConfig::new("de", 0, 10, 1, 2, 1, 32, 8, 12, 12).expect_err("repeat reject");
    assert_eq!(err.code(), "repeat.delay.invalid");
}

#[test]
fn test_reject_invalid_pointer_accel_config() {
    let err = InputdConfig::new("de", 100, 10, 4, 1, 1, 4, 8, 12, 12).expect_err("accel reject");
    assert_eq!(err.code(), "pointer_accel.max_output.invalid");
}

#[test]
fn test_reject_stale_windowd_route_target() {
    let server = WindowServer::new(WindowdConfig { width: 64, height: 48, hz: 60 }).expect("server");
    let mut inputd = InputdService::new(server, config(8)).expect("inputd");
    let mut hidrawd = HidrawdService::new();
    let mouse_id = DeviceId::new(2);
    hidrawd.register_mouse(mouse_id);
    let batch = hidrawd
        .ingest_mouse_report(mouse_id, TimestampNs::new(1), &[0, 1, 1])
        .expect("mouse batch");

    let err = inputd.apply_hid_batch(&batch).expect_err("route must reject without scene");
    assert_eq!(err.code(), "inputd.route.stale_surface");
    assert_eq!(err, InputdError::Route(windowd::WindowdError::StaleSurfaceId));
}

#[test]
fn test_reject_bounded_queue_overflow() {
    let (server, _caller, _surface) = fixture_server();
    let mut inputd = InputdService::new(server, config(1)).expect("inputd");
    let bounds = TouchBounds::new(128, 128).expect("bounds");
    let mut touchd = TouchdService::new(bounds, SyntheticTouchMode::Disabled);
    touchd.register_device(TouchDeviceId::new(9));
    let first = touchd
        .ingest(RawTouchSample::new(TouchTimestampNs::new(1), 10, 10, TouchPhase::Down))
        .expect("touch down");
    let second = touchd
        .ingest(RawTouchSample::new(TouchTimestampNs::new(2), 12, 12, TouchPhase::Move))
        .expect("touch move");

    inputd.apply_touch_event(first).expect("first dispatch");
    let err = inputd.apply_touch_event(second).expect_err("queue must reject overflow");
    assert_eq!(err.code(), "inputd.queue.overflow");
}

#[test]
fn touch_sequence_routes_through_windowd_authority() {
    let (server, caller, surface) = fixture_server();
    let mut inputd = InputdService::new(server, config(8)).expect("inputd");
    let bounds = TouchBounds::new(64, 48).expect("bounds");
    let mut touchd = TouchdService::new(bounds, SyntheticTouchMode::ProofFixture);
    touchd.register_device(TouchDeviceId::new(9));

    let touch_events = touchd.synthetic_sequence(1_000).expect("touch sequence");
    let mut dispatches = Vec::new();
    for event in touch_events {
        dispatches.push(inputd.apply_touch_event(event).expect("touch dispatch"));
    }

    assert_eq!(dispatches.len(), 3);
    assert!(matches!(dispatches[0], InputDispatch::Touch { x: 20, y: 20, .. }));
    assert!(matches!(dispatches[1], InputDispatch::Touch { x: 28, y: 22, .. }));
    assert!(matches!(dispatches[2], InputDispatch::Touch { x: 28, y: 22, .. }));

    let delivered = inputd
        .router_mut()
        .take_input_events(caller, surface)
        .expect("deliveries");
    assert_eq!(delivered.len(), 3);
    assert!(matches!(delivered[0].kind, windowd::InputEventKind::TouchDown { x: 20, y: 20 }));
    assert!(matches!(delivered[1].kind, windowd::InputEventKind::TouchMove { x: 28, y: 22 }));
    assert!(matches!(delivered[2].kind, windowd::InputEventKind::TouchUp { x: 28, y: 22 }));
    assert_eq!(inputd.router().focused_surface(), Some(surface));
}

#[test]
fn test_repeat_tick_is_deterministic_with_injected_time() {
    let (server, caller, surface) = fixture_server();
    let mut inputd = InputdService::new(server, config(16)).expect("inputd");

    let mut hidrawd = HidrawdService::new();
    let mouse_id = DeviceId::new(2);
    let keyboard_id = DeviceId::new(3);
    hidrawd.register_mouse(mouse_id);
    hidrawd.register_keyboard(keyboard_id);

    let pointer_batch = hidrawd
        .ingest_mouse_report(mouse_id, TimestampNs::new(1), &[0b001, 0, 0])
        .expect("pointer down batch");
    let pointer_dispatches = inputd.apply_hid_batch(&pointer_batch).expect("pointer route");
    assert!(matches!(pointer_dispatches.as_slice(), [InputDispatch::PointerDown { .. }]));

    let key_batch = hidrawd
        .ingest_keyboard_report(keyboard_id, TimestampNs::new(1), &[0, 0, 0x04, 0, 0, 0, 0, 0])
        .expect("keyboard batch");
    let key_dispatches = inputd.apply_hid_batch(&key_batch).expect("keyboard route");
    assert!(matches!(
        key_dispatches.as_slice(),
        [InputDispatch::Keyboard { repeated: false, .. }]
    ));

    let first_repeat = inputd.tick_repeat(100_000_001).expect("first repeat");
    let second_repeat = inputd.tick_repeat(200_000_001).expect("second repeat");
    assert_eq!(first_repeat.len(), 1);
    assert_eq!(second_repeat.len(), 1);
    assert!(matches!(
        first_repeat.as_slice(),
        [InputDispatch::Keyboard { repeated: true, .. }]
    ));
    assert!(matches!(
        second_repeat.as_slice(),
        [InputDispatch::Keyboard { repeated: true, .. }]
    ));

    let delivered = inputd
        .router_mut()
        .take_input_events(caller, surface)
        .expect("delivered events");
    assert_eq!(delivered.len(), 4);
    assert_eq!(delivered[0].kind, windowd::InputEventKind::PointerDown);
    assert!(matches!(delivered[1].kind, windowd::InputEventKind::Keyboard { key_code: 0x04 }));
    assert!(matches!(delivered[2].kind, windowd::InputEventKind::Keyboard { key_code: 0x04 }));
    assert!(matches!(delivered[3].kind, windowd::InputEventKind::Keyboard { key_code: 0x04 }));
}

#[test]
fn mouse_relative_motion_routes_through_windowd_authority() {
    let (server, caller, surface) = fixture_server();
    let mut inputd = InputdService::new(server, config(8)).expect("inputd");
    let mut hidrawd = HidrawdService::new();
    let mouse_id = DeviceId::new(4);
    hidrawd.register_mouse(mouse_id);

    let batch = hidrawd
        .ingest_mouse_report(
            mouse_id,
            TimestampNs::new(4),
            &[0b001, 2u8, (1i8) as u8],
        )
        .expect("mouse batch");
    let dispatches = inputd.apply_hid_batch(&batch).expect("dispatches");

    assert_eq!(dispatches.len(), 2);
    assert!(matches!(dispatches[0], InputDispatch::PointerMove { x: 15, y: 13, .. }));
    assert!(matches!(dispatches[1], InputDispatch::PointerDown { x: 15, y: 13, .. }));

    let delivered = inputd
        .router_mut()
        .take_input_events(caller, surface)
        .expect("deliveries");
    assert_eq!(delivered.len(), 2);
    assert!(matches!(delivered[0].kind, windowd::InputEventKind::PointerMove { .. }));
    assert_eq!(delivered[1].kind, windowd::InputEventKind::PointerDown);
}

#[test]
fn absolute_pointer_motion_routes_through_windowd_authority() {
    let (server, caller, surface) = fixture_server();
    let mut inputd = InputdService::new(server, config(8)).expect("inputd");
    let batch = hidrawd::HidBatch::new(
        DeviceId::new(5),
        hidrawd::HidDeviceKind::Mouse,
        vec![
            HidEvent::abs(TimestampNs::new(5), AbsoluteAxis::X.event_code(), 20),
            HidEvent::abs(TimestampNs::new(5), AbsoluteAxis::Y.event_code(), 10),
        ],
    );

    let dispatches = inputd.apply_hid_batch(&batch).expect("dispatches");
    assert_eq!(dispatches.len(), 1);
    assert!(matches!(dispatches[0], InputDispatch::PointerMove { x: 20, y: 10, .. }));

    let delivered = inputd
        .router_mut()
        .take_input_events(caller, surface)
        .expect("deliveries");
    assert_eq!(delivered.len(), 1);
    assert!(matches!(delivered[0].kind, windowd::InputEventKind::PointerMove { x: 20, y: 10 }));
}
