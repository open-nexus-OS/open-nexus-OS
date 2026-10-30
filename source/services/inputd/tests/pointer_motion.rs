// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The pointer's samples of a HID batch (RFC-0113 §2, TASK-0346 P4) through
//! `InputdService`: one sample per report (the run of events with one timestamp), its time and
//! the sequence of its first event; the relative step is the cursor's as before (the batch's
//! summed, accelerated motion), the samples lie along it and the last IS the cursor; an absolute
//! source's reports are its samples; a keyboard batch moves nothing.
//! OWNERS: @runtime

use hid::{AbsoluteAxis, RelativeAxis};
use input_live_protocol::{
    WireHidBatch, WireHidEvent, EVENT_KIND_ABS, EVENT_KIND_REL, HID_KIND_MOUSE,
    POINTER_SOURCE_MOUSE_RELATIVE, POINTER_SOURCE_TABLET_ABSOLUTE,
};
use inputd::{InputDispatch, InputdConfig, InputdService};
use windowd::{CallerCtx, CommitSeq, Layer, Rect, SurfaceBuffer, WindowServer, WindowdConfig};

fn server() -> WindowServer {
    let caller = CallerCtx::from_service_metadata(0x55);
    let (width, height) = (windowd::VISIBLE_BOOTSTRAP_WIDTH, windowd::VISIBLE_BOOTSTRAP_HEIGHT);
    let mut server = WindowServer::new(WindowdConfig { width, height, hz: 60 }).expect("server");
    let buffer =
        SurfaceBuffer::solid(caller, 50, width, height, [0x24, 0x28, 0x34, 0xff]).expect("buffer");
    let surface = server.create_surface(caller, buffer.clone()).expect("surface");
    server.queue_buffer(caller, surface, buffer, &[Rect::new(0, 0, width, height)]).expect("queue");
    let layer = Layer { surface, x: 0, y: 0, z: 0 };
    server.commit_scene(CallerCtx::system(), CommitSeq::new(1), &[layer]).expect("scene");
    let _ack = server.present_tick().expect("present tick").expect("present");
    server
}

fn inputd() -> InputdService<WindowServer> {
    let start = inputd::visible_display_start_position().expect("visible start");
    let display = inputd::visible_display_space().expect("visible display");
    let config = InputdConfig::new(
        "de",
        350,
        30,
        inputd::LIVE_POINTER_THRESHOLD,
        inputd::LIVE_POINTER_NUMERATOR,
        inputd::LIVE_POINTER_DENOMINATOR,
        inputd::LIVE_POINTER_MAX_OUTPUT,
        32,
        start.x,
        start.y,
    )
    .expect("config")
    .with_display_space(display.width(), display.height())
    .expect("display");
    InputdService::new(server(), config).expect("inputd")
}

fn ev(kind: u8, code: u16, value: i32, timestamp_ns: u64) -> WireHidEvent {
    WireHidEvent { kind, code, value, timestamp_ns }
}

fn batch(pointer_source: u8, seq_first: u32, events: Vec<WireHidEvent>) -> WireHidBatch {
    // An absolute source names its axes' range (decoded into display space).
    let abs_max = if pointer_source == POINTER_SOURCE_TABLET_ABSOLUTE { 1_000 } else { 0 };
    WireHidBatch {
        device_kind: HID_KIND_MOUSE,
        device_id: 9,
        pointer_source,
        abs_max_x: abs_max,
        abs_max_y: abs_max,
        raw_event_count: events.len() as u16,
        normalized_event_count: events.len() as u16,
        seq_first,
        events,
    }
}

fn apply(inputd: &mut InputdService<WindowServer>, wire: WireHidBatch) -> Vec<InputDispatch> {
    let hid = inputd::decode_wire_batch(wire, inputd.pointer_transform()).expect("wire decode");
    inputd.apply_hid_batch(&hid).expect("dispatches")
}

#[test]
fn each_report_of_a_relative_batch_is_a_sample_and_the_last_is_the_cursor() {
    let mut inputd = inputd();
    let start = inputd.display_pointer_position();
    let (x, y) = (RelativeAxis::X.event_code(), RelativeAxis::Y.event_code());
    let wire = batch(
        POINTER_SOURCE_MOUSE_RELATIVE,
        40,
        vec![
            ev(EVENT_KIND_REL, x, 2, 1_000),
            ev(EVENT_KIND_REL, y, 1, 1_000),
            ev(EVENT_KIND_REL, x, 2, 2_000),
            ev(EVENT_KIND_REL, x, 2, 3_000),
            ev(EVENT_KIND_REL, y, 1, 3_000),
        ],
    );
    let dispatches = apply(&mut inputd, wire);
    let Some(InputDispatch::PointerMove { x: cx, y: cy, .. }) = dispatches.first().cloned() else {
        panic!("one move: {dispatches:?}");
    };
    let samples = inputd.motion().samples().to_vec();
    assert_eq!(samples.len(), 3, "one sample per report");
    assert_eq!(samples.iter().map(|s| s.t_ns).collect::<Vec<_>>(), [1_000, 2_000, 3_000]);
    assert_eq!(samples.iter().map(|s| s.seq).collect::<Vec<_>>(), [40, 42, 43], "first events");
    assert_eq!((samples[2].x, samples[2].y), (cx, cy), "the last sample is the cursor");
    assert!(samples.windows(2).all(|w| w[0].x <= w[1].x), "along the step: {samples:?}");
    assert!(samples[0].x > start.x && samples[0].x < cx, "the first report is on the way");
    inputd.clear_motion();
    assert!(inputd.motion().is_empty());
}

#[test]
fn an_absolute_source_reports_its_positions() {
    let mut inputd = inputd();
    let (x, y) = (AbsoluteAxis::X.event_code(), AbsoluteAxis::Y.event_code());
    let wire = batch(
        POINTER_SOURCE_TABLET_ABSOLUTE,
        7,
        vec![
            ev(EVENT_KIND_ABS, x, 100, 10_000),
            ev(EVENT_KIND_ABS, y, 50, 10_000),
            ev(EVENT_KIND_ABS, x, 140, 20_000),
        ],
    );
    let dispatches = apply(&mut inputd, wire);
    let Some(InputDispatch::PointerMove { x: cx, y: cy, .. }) = dispatches.first().cloned() else {
        panic!("one move: {dispatches:?}");
    };
    let samples = inputd.motion().samples().to_vec();
    assert_eq!(samples.len(), 2);
    assert_eq!((samples[1].x, samples[1].y), (cx, cy), "the last report is the cursor");
    assert_eq!(samples[0].y, samples[1].y, "a report without y keeps the one before");
    assert!(samples[0].x < samples[1].x);
    assert_eq!((samples[0].seq, samples[1].seq), (7, 9));
}

#[test]
fn many_reports_fold_into_the_bound() {
    let mut inputd = inputd();
    let x = RelativeAxis::X.event_code();
    let events = (0..24).map(|i| ev(EVENT_KIND_REL, x, 1, 1_000 * (i + 1))).collect();
    let _ = apply(&mut inputd, batch(POINTER_SOURCE_MOUSE_RELATIVE, 0, events));
    let motion = inputd.motion();
    assert_eq!(motion.samples().len(), nexus_display_proto::surface_motion::MOTION_SAMPLES_MAX);
    assert_eq!(motion.newest().map(|s| s.t_ns), Some(24_000), "the newest report stays");
    assert_eq!(motion.samples()[0].t_ns, 9_000, "the oldest reports merged into one");
}
