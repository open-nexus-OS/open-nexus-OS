// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: keyboard-batch resilience (RFC-0075): fast typing packs several
//! keys into ONE batch (chunked hidraw drains) — a per-event failure must
//! never abort the whole batch.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Stable
//! TEST_COVERAGE: rollover, chord-skip, non-monotonic-timestamp survival

use hid::TimestampNs;
use hidrawd::{DeviceId, HidrawdService};
use inputd::{InputDispatch, InputdConfig, InputdService};
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

fn keyboard_count(d: &[InputDispatch]) -> usize {
    d.iter().filter(|x| matches!(x, InputDispatch::Keyboard { .. })).count()
}

#[test]
fn fast_typing_batch_survives_chords_and_time_regress() {
    // An unproducible key (Ctrl chord, unmapped usage) or a non-monotonic
    // hidraw timestamp must skip ITS event only — aborting the whole batch
    // ate every other key (the "fast typing loses input" report).
    let (server, caller, surface) = fixture_server();
    let config = InputdConfig::new("de", 100, 10, 1, 2, 1, 32, 16, 12, 12).expect("config");
    let mut inputd = InputdService::new(server, config).expect("inputd");
    let mut hidrawd = HidrawdService::new();
    let keyboard_id = DeviceId::new(9);
    let mouse_id = DeviceId::new(10);
    hidrawd.register_keyboard(keyboard_id);
    hidrawd.register_mouse(mouse_id);

    // Focus the surface (keyboard routing needs a focused target).
    let click = hidrawd
        .ingest_mouse_report(mouse_id, TimestampNs::new(1), &[0b001, 0, 0])
        .expect("focus click");
    inputd.apply_hid_batch(&click).expect("focus route");

    // Rollover: A+B land in one report — BOTH must dispatch.
    let batch = hidrawd
        .ingest_keyboard_report(keyboard_id, TimestampNs::new(100), &[0, 0, 0x04, 0x05, 0, 0, 0, 0])
        .expect("rollover batch");
    assert_eq!(keyboard_count(&inputd.apply_hid_batch(&batch).expect("rollover dispatches")), 2);

    // Timestamp REGRESSION: a new key stamped OLDER than the previous press
    // still delivers (repeat arming is best-effort, never batch-fatal).
    let batch = hidrawd
        .ingest_keyboard_report(
            keyboard_id,
            TimestampNs::new(50),
            &[0, 0, 0x04, 0x05, 0x06, 0, 0, 0],
        )
        .expect("regress batch");
    assert_eq!(keyboard_count(&inputd.apply_hid_batch(&batch).expect("regress dispatches")), 1);

    // A Ctrl chord the layout cannot produce skips its event, not the batch.
    let batch = hidrawd
        .ingest_keyboard_report(keyboard_id, TimestampNs::new(200), &[0x01, 0, 0x07, 0, 0, 0, 0, 0])
        .expect("chord batch");
    assert_eq!(keyboard_count(&inputd.apply_hid_batch(&batch).expect("chord survives")), 0);

    let _ = inputd.router_mut().take_input_events(caller, surface).expect("deliveries");
}
