// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: `inputd` behavior-first host tests for config validation and `windowd` routing.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Stable
//! TEST_COVERAGE: config rejects, bounded queue overflow, stale-route reject, repeat determinism
//! ADR: docs/adr/0029-input-v1-host-core-architecture.md

use hid::{AbsoluteAxis, HidEvent, RelativeAxis, TimestampNs};
use hidrawd::{
    normalize_ingress_batch, DeviceId, HidrawdService, IngressRole, PointerSource, RawIngressBatch,
    RawIngressEvent, RawIngressEventKind,
};
use input_live_protocol::{
    WireHidBatch, WireHidEvent, EVENT_KIND_ABS, EVENT_KIND_BTN, EVENT_KIND_KEY, EVENT_KIND_REL,
    HID_KIND_KEYBOARD, HID_KIND_MOUSE, POINTER_SOURCE_MOUSE_RELATIVE, POINTER_SOURCE_NONE,
    POINTER_SOURCE_TABLET_ABSOLUTE,
};
use inputd::{InputDispatch, InputdConfig, InputdError, InputdService};
use touch::{RawTouchSample, TouchBounds, TouchPhase, TouchTimestampNs};
use touchd::{SyntheticTouchMode, TouchDeviceId, TouchdService};
use windowd::{CallerCtx, CommitSeq, Layer, Rect, SurfaceBuffer, WindowServer, WindowdConfig};

fn fixture_server() -> (WindowServer, CallerCtx, windowd::SurfaceId) {
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
    (server, caller, surface)
}

// Full-display fixture at the production bootstrap resolution (1280×800), so the
// windowd router bounds (= inputd's route space) match the live display space. This
// mirrors the real path where inputd ships display-space pointer coordinates and
// windowd hit-tests them directly — no 64×48 route quantization (TASK #52).
fn full_surface_fixture_server() -> (WindowServer, CallerCtx, windowd::SurfaceId) {
    let caller = CallerCtx::from_service_metadata(0x55);
    let width = windowd::VISIBLE_BOOTSTRAP_WIDTH;
    let height = windowd::VISIBLE_BOOTSTRAP_HEIGHT;
    let mut server = WindowServer::new(WindowdConfig { width, height, hz: 60 }).expect("server");
    let buffer = SurfaceBuffer::solid(caller, 50, width, height, [0x24, 0x28, 0x34, 0xff])
        .expect("buffer");
    let surface = server.create_surface(caller, buffer.clone()).expect("surface");
    server
        .queue_buffer(caller, surface, buffer, &[Rect::new(0, 0, width, height)])
        .expect("queue");
    server
        .commit_scene(
            CallerCtx::system(),
            CommitSeq::new(1),
            &[Layer { surface, x: 0, y: 0, z: 0 }],
        )
        .expect("scene");
    let _ack = server.present_tick().expect("present tick").expect("present");
    (server, caller, surface)
}

fn config(queue_capacity: usize) -> InputdConfig {
    InputdConfig::new("de", 100, 10, 1, 2, 1, 32, queue_capacity, 12, 12).expect("config")
}

fn live_visible_config(queue_capacity: usize) -> InputdConfig {
    let start = inputd::visible_display_start_position().expect("visible start");
    let display = inputd::visible_display_space().expect("visible display");
    InputdConfig::new(
        "de",
        350,
        30,
        inputd::LIVE_POINTER_THRESHOLD,
        inputd::LIVE_POINTER_NUMERATOR,
        inputd::LIVE_POINTER_DENOMINATOR,
        inputd::LIVE_POINTER_MAX_OUTPUT,
        queue_capacity,
        start.x,
        start.y,
    )
    .expect("live visible config")
    .with_display_space(display.width(), display.height())
    .expect("live visible display config")
}

fn visible_pointer_wire_batch(
    pointer_source: u8,
    events: Vec<WireHidEvent>,
    abs_max_x: i32,
    abs_max_y: i32,
) -> WireHidBatch {
    WireHidBatch {
        device_kind: HID_KIND_MOUSE,
        device_id: 9,
        pointer_source,
        abs_max_x,
        abs_max_y,
        raw_event_count: events.len() as u16,
        normalized_event_count: events.len() as u16,
        events,
    }
}

fn normalize_wire_batch(
    service: &mut HidrawdService,
    device_id: DeviceId,
    raw_batch: &RawIngressBatch,
    timestamp_ns: u64,
    abs_max_x: i32,
    abs_max_y: i32,
) -> WireHidBatch {
    normalize_ingress_batch(
        service,
        device_id,
        raw_batch,
        TimestampNs::new(timestamp_ns),
        abs_max_x,
        abs_max_y,
    )
    .expect("ingress normalization")
    .into_wire_batch()
    .expect("wire batch")
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
    let server =
        WindowServer::new(WindowdConfig { width: 64, height: 48, hz: 60 }).expect("server");
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

    let delivered = inputd.router_mut().take_input_events(caller, surface).expect("deliveries");
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
    assert!(matches!(key_dispatches.as_slice(), [InputDispatch::Keyboard { repeated: false, .. }]));

    let first_repeat = inputd.tick_repeat(100_000_001).expect("first repeat");
    let second_repeat = inputd.tick_repeat(200_000_001).expect("second repeat");
    assert_eq!(first_repeat.len(), 1);
    assert_eq!(second_repeat.len(), 1);
    assert!(matches!(first_repeat.as_slice(), [InputDispatch::Keyboard { repeated: true, .. }]));
    assert!(matches!(second_repeat.as_slice(), [InputDispatch::Keyboard { repeated: true, .. }]));

    let delivered =
        inputd.router_mut().take_input_events(caller, surface).expect("delivered events");
    assert_eq!(delivered.len(), 4);
    assert_eq!(delivered[0].kind, windowd::InputEventKind::PointerDown);
    assert!(matches!(delivered[1].kind, windowd::InputEventKind::Keyboard { key_code: 0x04 }));
    assert!(matches!(delivered[2].kind, windowd::InputEventKind::Keyboard { key_code: 0x04 }));
    assert!(matches!(delivered[3].kind, windowd::InputEventKind::Keyboard { key_code: 0x04 }));
}

#[test]
fn mouse_relative_wire_batch_decodes_without_absolute_calibration() {
    let transform = inputd::visible_pointer_transform().expect("pointer transform");
    let hid_batch = inputd::decode_wire_batch(
        visible_pointer_wire_batch(
            POINTER_SOURCE_MOUSE_RELATIVE,
            vec![
                WireHidEvent { kind: EVENT_KIND_REL, code: 0, value: 5, timestamp_ns: 10 },
                WireHidEvent { kind: EVENT_KIND_BTN, code: 0x110, value: 1, timestamp_ns: 10 },
            ],
            0,
            0,
        ),
        transform,
    )
    .expect("mouse-relative wire batch");

    assert_eq!(hid_batch.kind(), hidrawd::HidDeviceKind::Mouse);
    assert_eq!(hid_batch.pointer_source(), Some(hidrawd::PointerSource::MouseRelative));
    assert_eq!(hid_batch.events().len(), 2);
    assert_eq!(hid_batch.events()[0].kind(), hid::HidEventKind::Rel);
    assert_eq!(hid_batch.events()[1].kind(), hid::HidEventKind::Btn);
}

#[test]
fn mouse_relative_raw_ingress_wire_pipeline_reaches_windowd_authority() {
    let (server, caller, surface) = fixture_server();
    let mut inputd = InputdService::new(server, config(8)).expect("inputd");
    let mut hidrawd = HidrawdService::new();
    let mouse_id = DeviceId::new(21);
    hidrawd.register_mouse(mouse_id);

    let wire_batch = normalize_wire_batch(
        &mut hidrawd,
        mouse_id,
        &RawIngressBatch::new(
            IngressRole::RelativePointer,
            vec![
                RawIngressEvent::new(
                    RawIngressEventKind::Relative,
                    RelativeAxis::X.event_code(),
                    2,
                ),
                RawIngressEvent::new(
                    RawIngressEventKind::Relative,
                    RelativeAxis::Y.event_code(),
                    1,
                ),
                RawIngressEvent::new(RawIngressEventKind::Key, 0x110, 1),
            ],
        ),
        90,
        0,
        0,
    );
    let hid_batch =
        inputd::decode_wire_batch(wire_batch, inputd.pointer_transform()).expect("wire decode");
    let dispatches = inputd.apply_hid_batch(&hid_batch).expect("dispatches");

    assert_eq!(dispatches.len(), 2);
    assert!(matches!(dispatches[0], InputDispatch::PointerMove { x: 15, y: 13, .. }));
    assert!(matches!(dispatches[1], InputDispatch::PointerDown { x: 15, y: 13, .. }));

    let delivered = inputd.router_mut().take_input_events(caller, surface).expect("deliveries");
    assert_eq!(delivered.len(), 2);
    assert!(matches!(delivered[0].kind, windowd::InputEventKind::PointerMove { x: 15, y: 13 }));
    assert_eq!(delivered[1].kind, windowd::InputEventKind::PointerDown);
}

#[test]
fn test_reject_tablet_absolute_wire_batch_without_calibration() {
    let transform = inputd::visible_pointer_transform().expect("pointer transform");
    let err = inputd::decode_wire_batch(
        visible_pointer_wire_batch(
            POINTER_SOURCE_TABLET_ABSOLUTE,
            vec![WireHidEvent { kind: EVENT_KIND_ABS, code: 0, value: 1024, timestamp_ns: 12 }],
            0,
            32767,
        ),
        transform,
    )
    .expect_err("tablet absolute without x calibration must reject");

    assert_eq!(err, inputd::WireBatchReject::InvalidAbsoluteCalibration(inputd::PointerAxis::X));
}

#[test]
fn test_reject_relative_event_on_tablet_absolute_source() {
    let transform = inputd::visible_pointer_transform().expect("pointer transform");
    let err = inputd::decode_wire_batch(
        visible_pointer_wire_batch(
            POINTER_SOURCE_TABLET_ABSOLUTE,
            vec![WireHidEvent { kind: EVENT_KIND_REL, code: 0, value: 7, timestamp_ns: 14 }],
            32767,
            32767,
        ),
        transform,
    )
    .expect_err("tablet source must reject relative motion");

    assert_eq!(
        err,
        inputd::WireBatchReject::RelativeOnAbsoluteSource(hidrawd::PointerSource::TabletAbsolute)
    );
}

#[test]
fn tablet_absolute_wire_batch_accepts_relative_wheel_events_for_scroll() {
    let transform = inputd::visible_pointer_transform().expect("pointer transform");
    let hid_batch = inputd::decode_wire_batch(
        visible_pointer_wire_batch(
            POINTER_SOURCE_TABLET_ABSOLUTE,
            vec![WireHidEvent {
                kind: EVENT_KIND_REL,
                code: RelativeAxis::Wheel.event_code(),
                value: -1,
                timestamp_ns: 15,
            }],
            32767,
            32767,
        ),
        transform,
    )
    .expect("tablet source must accept wheel");

    assert_eq!(hid_batch.pointer_source(), Some(hidrawd::PointerSource::TabletAbsolute));
    assert_eq!(hid_batch.events().len(), 1);
    assert_eq!(hid_batch.events()[0].kind(), hid::HidEventKind::Rel);
    assert_eq!(hid_batch.events()[0].code().raw(), RelativeAxis::Wheel.event_code());
    assert_eq!(hid_batch.events()[0].value().raw(), -1);
}

#[test]
fn keyboard_wire_batch_decodes_without_pointer_source() {
    let transform = inputd::visible_pointer_transform().expect("pointer transform");
    let hid_batch = inputd::decode_wire_batch(
        WireHidBatch {
            device_kind: HID_KIND_KEYBOARD,
            device_id: 4,
            pointer_source: POINTER_SOURCE_NONE,
            abs_max_x: 0,
            abs_max_y: 0,
            raw_event_count: 1,
            normalized_event_count: 1,
            events: vec![WireHidEvent {
                kind: EVENT_KIND_KEY,
                code: 0x04,
                value: 1,
                timestamp_ns: 16,
            }],
        },
        transform,
    )
    .expect("keyboard wire batch");

    assert_eq!(hid_batch.kind(), hidrawd::HidDeviceKind::Keyboard);
    assert_eq!(hid_batch.pointer_source(), None);
    assert_eq!(hid_batch.events().len(), 1);
    assert_eq!(hid_batch.events()[0].kind(), hid::HidEventKind::Key);
}

#[test]
fn tablet_absolute_wire_batch_preserves_pointer_source_identity() {
    let transform = inputd::visible_pointer_transform().expect("pointer transform");
    let hid_batch = inputd::decode_wire_batch(
        visible_pointer_wire_batch(
            POINTER_SOURCE_TABLET_ABSOLUTE,
            vec![
                WireHidEvent { kind: EVENT_KIND_ABS, code: 0, value: 16_384, timestamp_ns: 18 },
                WireHidEvent { kind: EVENT_KIND_ABS, code: 1, value: 8_192, timestamp_ns: 18 },
            ],
            32_767,
            32_767,
        ),
        transform,
    )
    .expect("tablet wire batch");

    assert_eq!(hid_batch.pointer_source(), Some(hidrawd::PointerSource::TabletAbsolute));
}

#[test]
fn tablet_absolute_raw_ingress_wire_pipeline_reaches_windowd_authority() {
    let (server, caller, surface) = fixture_server();
    let mut inputd = InputdService::new(server, config(8)).expect("inputd");
    let mut hidrawd = HidrawdService::new();
    let pointer_id = DeviceId::new(22);
    hidrawd.register_mouse(pointer_id);

    let wire_batch = normalize_wire_batch(
        &mut hidrawd,
        pointer_id,
        &RawIngressBatch::with_pointer_source(
            IngressRole::AbsolutePointer,
            Some(PointerSource::TabletAbsolute),
            vec![
                RawIngressEvent::new(
                    RawIngressEventKind::Absolute,
                    AbsoluteAxis::X.event_code(),
                    20,
                ),
                RawIngressEvent::new(
                    RawIngressEventKind::Absolute,
                    AbsoluteAxis::Y.event_code(),
                    10,
                ),
            ],
        ),
        91,
        63,
        47,
    );
    let hid_batch =
        inputd::decode_wire_batch(wire_batch, inputd.pointer_transform()).expect("wire decode");
    let dispatches = inputd.apply_hid_batch(&hid_batch).expect("dispatches");

    assert!(matches!(dispatches.as_slice(), [InputDispatch::PointerMove { x: 20, y: 10, .. }]));
    assert_eq!(inputd.active_pointer_source(), Some(PointerSource::TabletAbsolute));

    let delivered = inputd.router_mut().take_input_events(caller, surface).expect("deliveries");
    assert_eq!(delivered.len(), 1);
    assert!(matches!(delivered[0].kind, windowd::InputEventKind::PointerMove { x: 20, y: 10 }));
}

#[test]
fn tablet_absolute_raw_ingress_pipeline_blocks_following_relative_mouse_batches() {
    let (server, _caller, _surface) = full_surface_fixture_server();
    let mut inputd = InputdService::new(server, live_visible_config(8)).expect("inputd");
    let transform = inputd::visible_pointer_transform().expect("visible transform");
    let target_display = transform.route_to_display(inputd::PointerPosition::new(
        inputd::VISIBLE_INPUT_CURSOR_END_X,
        inputd::VISIBLE_INPUT_CURSOR_END_Y,
    ));
    let mut hidrawd = HidrawdService::new();
    let pointer_id = DeviceId::new(23);
    hidrawd.register_mouse(pointer_id);

    let tablet_wire = normalize_wire_batch(
        &mut hidrawd,
        pointer_id,
        &RawIngressBatch::with_pointer_source(
            IngressRole::AbsolutePointer,
            Some(PointerSource::TabletAbsolute),
            vec![
                RawIngressEvent::new(
                    RawIngressEventKind::Absolute,
                    AbsoluteAxis::X.event_code(),
                    target_display.x,
                ),
                RawIngressEvent::new(
                    RawIngressEventKind::Absolute,
                    AbsoluteAxis::Y.event_code(),
                    target_display.y,
                ),
            ],
        ),
        92,
        1279,
        799,
    );
    let tablet_batch =
        inputd::decode_wire_batch(tablet_wire, inputd.pointer_transform()).expect("tablet decode");
    let tablet_dispatches = inputd.apply_hid_batch(&tablet_batch).expect("tablet dispatches");
    assert!(matches!(
        tablet_dispatches.as_slice(),
        [InputDispatch::PointerMove { x, y, .. }]
            if (*x, *y) == (target_display.x, target_display.y)
    ));

    let relative_wire = normalize_wire_batch(
        &mut hidrawd,
        pointer_id,
        &RawIngressBatch::new(
            IngressRole::RelativePointer,
            vec![
                RawIngressEvent::new(
                    RawIngressEventKind::Relative,
                    RelativeAxis::X.event_code(),
                    40,
                ),
                RawIngressEvent::new(
                    RawIngressEventKind::Relative,
                    RelativeAxis::Y.event_code(),
                    -20,
                ),
            ],
        ),
        93,
        0,
        0,
    );
    let relative_batch = inputd::decode_wire_batch(relative_wire, inputd.pointer_transform())
        .expect("relative decode");
    let relative_dispatches = inputd.apply_hid_batch(&relative_batch).expect("relative dispatches");

    assert!(relative_dispatches.is_empty());
    assert_eq!(inputd.display_pointer_position(), target_display);
    assert_eq!(inputd.active_pointer_source(), Some(PointerSource::TabletAbsolute));
}

#[test]
fn tablet_absolute_source_blocks_following_relative_motion_in_live_mixed_stream() {
    let (server, _caller, _surface) = full_surface_fixture_server();
    let mut inputd = InputdService::new(server, live_visible_config(8)).expect("inputd");
    let transform = inputd::visible_pointer_transform().expect("visible transform");
    let target_display = transform.route_to_display(inputd::PointerPosition::new(
        inputd::VISIBLE_INPUT_CURSOR_END_X,
        inputd::VISIBLE_INPUT_CURSOR_END_Y,
    ));
    let absolute_batch = hidrawd::HidBatch::new_pointer(
        DeviceId::new(8),
        hidrawd::PointerSource::TabletAbsolute,
        vec![
            HidEvent::abs(TimestampNs::new(30), AbsoluteAxis::X.event_code(), target_display.x),
            HidEvent::abs(TimestampNs::new(30), AbsoluteAxis::Y.event_code(), target_display.y),
        ],
    );
    let relative_batch = hidrawd::HidBatch::new_pointer(
        DeviceId::new(9),
        hidrawd::PointerSource::MouseRelative,
        vec![
            HidEvent::rel(TimestampNs::new(31), RelativeAxis::X.event_code(), 40),
            HidEvent::rel(TimestampNs::new(31), RelativeAxis::Y.event_code(), -20),
        ],
    );

    let absolute_dispatches = inputd.apply_hid_batch(&absolute_batch).expect("absolute dispatches");
    assert!(matches!(
        absolute_dispatches.as_slice(),
        [InputDispatch::PointerMove { x, y, .. }]
            if (*x, *y) == (target_display.x, target_display.y)
    ));
    let relative_dispatches = inputd.apply_hid_batch(&relative_batch).expect("relative dispatches");

    assert!(relative_dispatches.is_empty());
    assert_eq!(inputd.display_pointer_position(), target_display);
    assert_eq!(inputd.active_pointer_source(), Some(hidrawd::PointerSource::TabletAbsolute));
}

#[test]
fn tablet_absolute_source_supports_sustained_live_motion_across_multiple_batches() {
    let (server, _caller, _surface) = full_surface_fixture_server();
    let mut inputd = InputdService::new(server, live_visible_config(16)).expect("inputd");
    let transform = inputd::visible_pointer_transform().expect("visible transform");
    let route_steps = [
        inputd::PointerPosition::new(24, 12),
        inputd::PointerPosition::new(20, 20),
        inputd::PointerPosition::new(12, 28),
        inputd::PointerPosition::new(8, 40),
    ];

    for (idx, route) in route_steps.into_iter().enumerate() {
        let display = transform.route_to_display(route);
        let batch = hidrawd::HidBatch::new_pointer(
            DeviceId::new(10),
            hidrawd::PointerSource::TabletAbsolute,
            vec![
                HidEvent::abs(
                    TimestampNs::new(40 + idx as u64),
                    AbsoluteAxis::X.event_code(),
                    display.x,
                ),
                HidEvent::abs(
                    TimestampNs::new(40 + idx as u64),
                    AbsoluteAxis::Y.event_code(),
                    display.y,
                ),
            ],
        );

        let dispatches = inputd.apply_hid_batch(&batch).expect("dispatches");
        assert!(matches!(
            dispatches.as_slice(),
            [InputDispatch::PointerMove { x, y, .. }] if (*x, *y) == (display.x, display.y)
        ));
        assert_eq!(inputd.display_pointer_position(), display);
        assert_eq!(inputd.active_pointer_source(), Some(hidrawd::PointerSource::TabletAbsolute));
    }
}

#[test]
fn mouse_relative_motion_routes_through_windowd_authority() {
    let (server, caller, surface) = fixture_server();
    let mut inputd = InputdService::new(server, config(8)).expect("inputd");
    let mut hidrawd = HidrawdService::new();
    let mouse_id = DeviceId::new(4);
    hidrawd.register_mouse(mouse_id);

    let batch = hidrawd
        .ingest_mouse_report(mouse_id, TimestampNs::new(4), &[0b001, 2u8, (1i8) as u8])
        .expect("mouse batch");
    let dispatches = inputd.apply_hid_batch(&batch).expect("dispatches");

    assert_eq!(dispatches.len(), 2);
    assert!(matches!(dispatches[0], InputDispatch::PointerMove { x: 15, y: 13, .. }));
    assert!(matches!(dispatches[1], InputDispatch::PointerDown { x: 15, y: 13, .. }));

    let delivered = inputd.router_mut().take_input_events(caller, surface).expect("deliveries");
    assert_eq!(delivered.len(), 2);
    assert!(matches!(delivered[0].kind, windowd::InputEventKind::PointerMove { .. }));
    assert_eq!(delivered[1].kind, windowd::InputEventKind::PointerDown);
}

#[test]
fn mouse_primary_button_hold_tracks_press_until_release() {
    let (server, caller, surface) = fixture_server();
    let mut inputd = InputdService::new(server, config(8)).expect("inputd");

    let press = hidrawd::HidBatch::new(
        DeviceId::new(4),
        hidrawd::HidDeviceKind::Mouse,
        vec![HidEvent::btn(TimestampNs::new(10), 0x110, 1)],
    );
    let press_dispatches = inputd.apply_hid_batch(&press).expect("press dispatches");
    assert!(matches!(
        press_dispatches.as_slice(),
        [InputDispatch::PointerDown { x: 12, y: 12, .. }]
    ));
    assert!(inputd.primary_pointer_held());

    let release = hidrawd::HidBatch::new(
        DeviceId::new(4),
        hidrawd::HidDeviceKind::Mouse,
        vec![HidEvent::btn(TimestampNs::new(11), 0x110, 0)],
    );
    let release_dispatches = inputd.apply_hid_batch(&release).expect("release dispatches");
    assert!(release_dispatches.is_empty());
    assert!(!inputd.primary_pointer_held());

    let delivered = inputd.router_mut().take_input_events(caller, surface).expect("deliveries");
    assert_eq!(delivered.len(), 1);
    assert_eq!(delivered[0].kind, windowd::InputEventKind::PointerDown);
}

#[test]
fn mouse_wheel_batches_emit_pointer_wheel_dispatch_without_pointer_motion() {
    let (server, _caller, _surface) = fixture_server();
    let mut inputd = InputdService::new(server, config(8)).expect("inputd");

    let wheel = hidrawd::HidBatch::new_pointer(
        DeviceId::new(4),
        hidrawd::PointerSource::MouseRelative,
        vec![HidEvent::rel(TimestampNs::new(12), RelativeAxis::Wheel.event_code(), -1)],
    );
    let dispatches = inputd.apply_hid_batch(&wheel).expect("wheel dispatches");
    assert!(matches!(dispatches.as_slice(), [InputDispatch::PointerWheel { delta_y: -1 }]));
    assert_eq!(inputd.display_pointer_position().x, 12);
    assert_eq!(inputd.display_pointer_position().y, 12);
}

#[test]
fn live_fastpath_pointer_burst_coalesces_but_click_and_keyboard_edges_still_route() {
    let (mut server, caller, surface) = full_surface_fixture_server();
    server.enable_fastpath();
    let mut inputd = InputdService::new(server, live_visible_config(16)).expect("inputd");

    for step in 0..3u64 {
        let batch = hidrawd::HidBatch::new_pointer(
            DeviceId::new(4),
            hidrawd::PointerSource::MouseRelative,
            vec![HidEvent::rel(TimestampNs::new(30 + step), RelativeAxis::X.event_code(), 1)],
        );
        let dispatches = inputd.apply_hid_batch(&batch).expect("move dispatches");
        assert!(matches!(
            dispatches.as_slice(),
            [InputDispatch::PointerMove { delivery, .. }] if delivery.seq.raw() == 0
        ));
    }

    assert_eq!(inputd.router().pointer_coalesce_burst(), 3);
    let delivered_moves =
        inputd.router_mut().take_input_events(caller, surface).expect("move deliveries");
    assert!(
        delivered_moves.is_empty(),
        "coalesced pointer burst should not enqueue per-move deliveries"
    );

    let press = hidrawd::HidBatch::new(
        DeviceId::new(4),
        hidrawd::HidDeviceKind::Mouse,
        vec![HidEvent::btn(TimestampNs::new(40), 0x110, 1)],
    );
    let press_dispatches = inputd.apply_hid_batch(&press).expect("press dispatches");
    assert!(matches!(
        press_dispatches.as_slice(),
        [InputDispatch::PointerDown { delivery, .. }] if delivery.surface == surface
    ));
    assert_eq!(
        inputd.router().pointer_coalesce_burst(),
        0,
        "click edge should reset pointer burst"
    );

    let delivered_press =
        inputd.router_mut().take_input_events(caller, surface).expect("press deliveries");
    assert_eq!(delivered_press.len(), 1);
    assert_eq!(delivered_press[0].kind, windowd::InputEventKind::PointerDown);

    let key = hidrawd::HidBatch::new(
        DeviceId::new(5),
        hidrawd::HidDeviceKind::Keyboard,
        vec![HidEvent::key(TimestampNs::new(41), 0x04, 1)],
    );
    let key_dispatches = inputd.apply_hid_batch(&key).expect("key dispatches");
    assert!(matches!(
        key_dispatches.as_slice(),
        [InputDispatch::Keyboard { delivery, key_code: 0x04, repeated: false, .. }]
            if delivery.surface == surface
    ));

    let delivered_key =
        inputd.router_mut().take_input_events(caller, surface).expect("key deliveries");
    assert_eq!(delivered_key.len(), 1);
    assert!(matches!(delivered_key[0].kind, windowd::InputEventKind::Keyboard { key_code: 0x04 }));
}

#[test]
fn keyboard_hold_count_tracks_non_modifier_keys_until_last_release() {
    let (server, _caller, _surface) = fixture_server();
    let mut inputd = InputdService::new(server, config(8)).expect("inputd");

    let focus_batch = hidrawd::HidBatch::new(
        DeviceId::new(4),
        hidrawd::HidDeviceKind::Mouse,
        vec![HidEvent::btn(TimestampNs::new(20), 0x110, 1)],
    );
    inputd.apply_hid_batch(&focus_batch).expect("focus dispatch");

    let press_a = hidrawd::HidBatch::new(
        DeviceId::new(5),
        hidrawd::HidDeviceKind::Keyboard,
        vec![HidEvent::key(TimestampNs::new(21), 0x04, 1)],
    );
    let press_a_dispatches = inputd.apply_hid_batch(&press_a).expect("press a");
    assert!(matches!(
        press_a_dispatches.as_slice(),
        [InputDispatch::Keyboard { key_code: 0x04, repeated: false, .. }]
    ));
    assert_eq!(inputd.held_non_modifier_key_count(), 1);

    let press_shift = hidrawd::HidBatch::new(
        DeviceId::new(5),
        hidrawd::HidDeviceKind::Keyboard,
        vec![HidEvent::key(TimestampNs::new(22), 0xe1, 1)],
    );
    let press_shift_dispatches = inputd.apply_hid_batch(&press_shift).expect("press shift");
    assert!(press_shift_dispatches.is_empty());
    assert_eq!(inputd.held_non_modifier_key_count(), 1);

    let press_b = hidrawd::HidBatch::new(
        DeviceId::new(5),
        hidrawd::HidDeviceKind::Keyboard,
        vec![HidEvent::key(TimestampNs::new(23), 0x05, 1)],
    );
    let press_b_dispatches = inputd.apply_hid_batch(&press_b).expect("press b");
    assert!(matches!(
        press_b_dispatches.as_slice(),
        [InputDispatch::Keyboard { key_code: 0x05, repeated: false, .. }]
    ));
    assert_eq!(inputd.held_non_modifier_key_count(), 2);

    let release_a = hidrawd::HidBatch::new(
        DeviceId::new(5),
        hidrawd::HidDeviceKind::Keyboard,
        vec![HidEvent::key(TimestampNs::new(24), 0x04, 0)],
    );
    let release_a_dispatches = inputd.apply_hid_batch(&release_a).expect("release a");
    assert!(release_a_dispatches.is_empty());
    assert_eq!(inputd.held_non_modifier_key_count(), 1);

    let release_shift = hidrawd::HidBatch::new(
        DeviceId::new(5),
        hidrawd::HidDeviceKind::Keyboard,
        vec![HidEvent::key(TimestampNs::new(25), 0xe1, 0)],
    );
    let release_shift_dispatches = inputd.apply_hid_batch(&release_shift).expect("release shift");
    assert!(release_shift_dispatches.is_empty());
    assert_eq!(inputd.held_non_modifier_key_count(), 1);

    let release_b = hidrawd::HidBatch::new(
        DeviceId::new(5),
        hidrawd::HidDeviceKind::Keyboard,
        vec![HidEvent::key(TimestampNs::new(26), 0x05, 0)],
    );
    let release_b_dispatches = inputd.apply_hid_batch(&release_b).expect("release b");
    assert!(release_b_dispatches.is_empty());
    assert_eq!(inputd.held_non_modifier_key_count(), 0);
}

#[test]
fn relative_pointer_motion_preserves_screen_direction_contract() {
    let (server, caller, surface) = full_surface_fixture_server();
    let mut inputd = InputdService::new(server, config(8)).expect("inputd");
    let steps = [
        (RelativeAxis::X, 1, (13, 12)),
        (RelativeAxis::Y, 1, (13, 13)),
        (RelativeAxis::X, -1, (12, 13)),
        (RelativeAxis::Y, -1, (12, 12)),
    ];

    for (idx, (axis, delta, expected)) in steps.into_iter().enumerate() {
        let batch = hidrawd::HidBatch::new(
            DeviceId::new(4),
            hidrawd::HidDeviceKind::Mouse,
            vec![HidEvent::rel(TimestampNs::new(10 + idx as u64), axis.event_code(), delta)],
        );
        let dispatches = inputd.apply_hid_batch(&batch).expect("dispatches");
        assert!(matches!(
            dispatches.as_slice(),
            [InputDispatch::PointerMove { x, y, .. }] if (*x, *y) == expected
        ));
    }

    let delivered = inputd.router_mut().take_input_events(caller, surface).expect("deliveries");
    assert_eq!(delivered.len(), 4);
    assert!(matches!(delivered[0].kind, windowd::InputEventKind::PointerMove { x: 13, y: 12 }));
    assert!(matches!(delivered[1].kind, windowd::InputEventKind::PointerMove { x: 13, y: 13 }));
    assert!(matches!(delivered[2].kind, windowd::InputEventKind::PointerMove { x: 12, y: 13 }));
    assert!(matches!(delivered[3].kind, windowd::InputEventKind::PointerMove { x: 12, y: 12 }));
}

#[test]
fn live_visible_pointer_speed_reaches_hover_target_without_edge_clamp() {
    let (server, caller, surface) = full_surface_fixture_server();
    let mut inputd = InputdService::new(server, live_visible_config(8)).expect("inputd");
    let target_display = inputd::visible_pointer_transform()
        .expect("visible transform")
        .route_to_display(inputd::PointerPosition::new(
            inputd::VISIBLE_INPUT_CURSOR_END_X,
            inputd::VISIBLE_INPUT_CURSOR_END_Y,
        ));
    let batch = hidrawd::HidBatch::new(
        DeviceId::new(4),
        hidrawd::HidDeviceKind::Mouse,
        vec![
            HidEvent::abs(TimestampNs::new(20), AbsoluteAxis::X.event_code(), target_display.x),
            HidEvent::abs(TimestampNs::new(20), AbsoluteAxis::Y.event_code(), target_display.y),
        ],
    );

    let dispatches = inputd.apply_hid_batch(&batch).expect("dispatches");
    assert!(matches!(
        dispatches.as_slice(),
        [InputDispatch::PointerMove { x, y, .. }]
            if (*x, *y) == (target_display.x, target_display.y)
    ));
    assert_eq!(inputd.display_pointer_position(), target_display);
    // Hit-testing moved to windowd (the compositor owns it); inputd only proves
    // pointer-accel reaches the target display position without edge clamping.

    let delivered = inputd.router_mut().take_input_events(caller, surface).expect("deliveries");
    assert_eq!(delivered.len(), 1);
    assert!(matches!(
        delivered[0].kind,
        windowd::InputEventKind::PointerMove { x, y }
            if (x, y) == (target_display.x, target_display.y)
    ));
}

#[test]
fn absolute_pointer_scaling_covers_the_full_visible_input_area() {
    let transform = inputd::visible_pointer_transform().expect("visible transform");
    let calibration = inputd::AbsoluteAxisCalibration::new(0, 32_767).expect("calibration");
    assert_eq!(transform.scale_absolute_axis(0, calibration, inputd::PointerAxis::X), 0);
    assert_eq!(transform.scale_absolute_axis(32_767, calibration, inputd::PointerAxis::X), 1279);
    assert_eq!(transform.scale_absolute_axis(0, calibration, inputd::PointerAxis::Y), 0);
    assert_eq!(transform.scale_absolute_axis(32_767, calibration, inputd::PointerAxis::Y), 799);

    let center_x = transform.scale_absolute_axis(16_384, calibration, inputd::PointerAxis::X);
    let center_y = transform.scale_absolute_axis(16_384, calibration, inputd::PointerAxis::Y);
    assert!((639..=640).contains(&center_x));
    assert!((399..=400).contains(&center_y));
}

#[test]
fn relative_pointer_motion_clamps_at_window_bounds_instead_of_dropping_live_motion() {
    let (server, caller, surface) = full_surface_fixture_server();
    let mut inputd = InputdService::new(server, config(8)).expect("inputd");
    let batch = hidrawd::HidBatch::new(
        DeviceId::new(4),
        hidrawd::HidDeviceKind::Mouse,
        vec![HidEvent::rel(TimestampNs::new(5), RelativeAxis::Y.event_code(), -100)],
    );

    let dispatches = inputd.apply_hid_batch(&batch).expect("dispatches");
    assert!(matches!(dispatches.as_slice(), [InputDispatch::PointerMove { x: 12, y: 0, .. }]));

    let delivered = inputd.router_mut().take_input_events(caller, surface).expect("deliveries");
    assert_eq!(delivered.len(), 1);
    assert!(matches!(delivered[0].kind, windowd::InputEventKind::PointerMove { x: 12, y: 0 }));
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

    let delivered = inputd.router_mut().take_input_events(caller, surface).expect("deliveries");
    assert_eq!(delivered.len(), 1);
    assert!(matches!(delivered[0].kind, windowd::InputEventKind::PointerMove { x: 20, y: 10 }));
}

#[test]
fn absolute_pointer_split_axes_route_each_update_using_last_position() {
    let (server, caller, surface) = fixture_server();
    let mut inputd = InputdService::new(server, config(8)).expect("inputd");

    let x_batch = hidrawd::HidBatch::new(
        DeviceId::new(6),
        hidrawd::HidDeviceKind::Mouse,
        vec![HidEvent::abs(TimestampNs::new(6), AbsoluteAxis::X.event_code(), 20)],
    );
    let x_dispatches = inputd.apply_hid_batch(&x_batch).expect("x dispatch");
    assert!(matches!(x_dispatches.as_slice(), [InputDispatch::PointerMove { x: 20, y: 12, .. }]));

    let y_batch = hidrawd::HidBatch::new(
        DeviceId::new(6),
        hidrawd::HidDeviceKind::Mouse,
        vec![HidEvent::abs(TimestampNs::new(7), AbsoluteAxis::Y.event_code(), 16)],
    );
    let y_dispatches = inputd.apply_hid_batch(&y_batch).expect("y dispatch");
    assert!(matches!(y_dispatches.as_slice(), [InputDispatch::PointerMove { x: 20, y: 16, .. }]));

    let delivered = inputd.router_mut().take_input_events(caller, surface).expect("deliveries");
    assert_eq!(delivered.len(), 2);
    assert!(matches!(delivered[0].kind, windowd::InputEventKind::PointerMove { x: 20, y: 12 }));
    assert!(matches!(delivered[1].kind, windowd::InputEventKind::PointerMove { x: 20, y: 16 }));
}
