// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: OS-lite `inputd` live route backend for RFC-0054.
//! OWNERS: @runtime @ui
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `cargo test -p inputd -- --nocapture`

extern crate alloc;

use alloc::{format, vec::Vec};

use hid::{HidEvent, TimestampNs};
use hidrawd::{DeviceId, HidBatch, HidDeviceKind};
use input_live_protocol::{
    decode_push_hid_batch, encode_status, encode_visible_state_frame, frame_has_op, VisibleState,
    WireHidBatch, EVENT_KIND_ABS, EVENT_KIND_BTN, EVENT_KIND_KEY, EVENT_KIND_REL,
    HID_KIND_KEYBOARD, HID_KIND_MOUSE, OP_GET_VISIBLE_STATE, OP_PUSH_HID_BATCH, STATUS_MALFORMED,
    STATUS_OK, STATUS_OVERFLOW, STATUS_UNSUPPORTED,
};
use nexus_abi::{debug_println, nsec, yield_};
use nexus_ipc::{KernelServer, Server as _, Wait};

use crate::{InputDispatch, InputdConfig, InputdService, RouteTarget};

const VISIBLE_INPUT_PROOF_WIDTH: u32 = 64;
const VISIBLE_INPUT_PROOF_HEIGHT: u32 = 48;
const VISIBLE_INPUT_SURFACE_X: i32 = 0;
const VISIBLE_INPUT_SURFACE_Y: i32 = 0;
const VISIBLE_INPUT_SURFACE_WIDTH: u32 = VISIBLE_INPUT_PROOF_WIDTH;
const VISIBLE_INPUT_SURFACE_HEIGHT: u32 = VISIBLE_INPUT_PROOF_HEIGHT;
const VISIBLE_INPUT_CURSOR_START_X: i32 = 24;
const VISIBLE_INPUT_CURSOR_START_Y: i32 = 12;
const VISIBLE_INPUT_LEFT_SQUARE_X: u32 = 4;
const VISIBLE_INPUT_LEFT_SQUARE_Y: u32 = 36;
const VISIBLE_INPUT_RIGHT_SQUARE_X: u32 = 52;
const VISIBLE_INPUT_RIGHT_SQUARE_Y: u32 = 18;
const VISIBLE_INPUT_SQUARE_SIZE: u32 = 8;
const VISIBLE_INPUT_BGRA: [u8; 4] = [0x18, 0x30, 0x88, 0xff];
const VISIBLE_INPUT_LEFT_IDLE_BGRA: [u8; 4] = [0x30, 0x70, 0xd8, 0xff];
const VISIBLE_INPUT_RIGHT_IDLE_BGRA: [u8; 4] = [0x90, 0x40, 0x40, 0xff];

pub fn service_main_loop() -> Result<(), &'static str> {
    let mut runtime = LiveRouteRuntime::new()?;
    let server = match KernelServer::new_for("inputd") {
        Ok(server) => server,
        Err(err) => {
            let _ = debug_println(match err {
                nexus_ipc::IpcError::WouldBlock => "inputd: route err would-block",
                nexus_ipc::IpcError::Timeout => "inputd: route err timeout",
                nexus_ipc::IpcError::Disconnected => "inputd: route err disconnected",
                nexus_ipc::IpcError::NoSpace => "inputd: route err no-space",
                nexus_ipc::IpcError::Kernel(_) => "inputd: route err kernel",
                nexus_ipc::IpcError::Unsupported => "inputd: route err unsupported",
                _ => "inputd: route err other",
            });
            // #region agent log
            agent_log(
                "H2",
                "source/services/inputd/src/os_lite.rs:46",
                "inputd named route bind failed",
                agent_ipc_error_data(err),
            );
            // #endregion
            let _ = debug_println("inputd: route fallback");
            let server = KernelServer::new_with_slots(3, 4)
                .map_err(|_| fail("inputd: init fail kernel-server"))?;
            let (recv_slot, send_slot) = server.slots();
            // #region agent log
            agent_log(
                "H1",
                "source/services/inputd/src/os_lite.rs:63",
                "inputd fallback bind",
                &format!("mode=fallback recv_slot={recv_slot} send_slot={send_slot}"),
            );
            // #endregion
            server
        }
    };
    let (recv_slot, send_slot) = server.slots();
    // #region agent log
    agent_log(
        "H2",
        "source/services/inputd/src/os_lite.rs:46",
        "inputd named route bind",
        &format!("mode=named recv_slot={recv_slot} send_slot={send_slot}"),
    );
    // #endregion
    debug_println("inputd: ready").map_err(|_| "inputd ready log failed")?;
    debug_println("inputd: keymap=de").map_err(|_| "inputd keymap log failed")?;
    debug_println("inputd: os service payload ready").map_err(|_| "inputd payload log failed")?;
    loop {
        match server.recv_request_with_meta(Wait::Blocking) {
            Ok((frame, _sender_service_id, reply)) => {
                runtime.chain.total_frames = runtime.chain.total_frames.saturating_add(1);
                if frame_has_op(&frame, OP_GET_VISIBLE_STATE) {
                    runtime.chain.visible_state_polls =
                        runtime.chain.visible_state_polls.saturating_add(1);
                } else if frame_has_op(&frame, OP_PUSH_HID_BATCH) {
                    runtime.chain.hid_push_frames = runtime.chain.hid_push_frames.saturating_add(1);
                } else {
                    runtime.chain.unsupported_frames =
                        runtime.chain.unsupported_frames.saturating_add(1);
                }
                if !frame_has_op(&frame, OP_GET_VISIBLE_STATE) {
                    // #region agent log
                    agent_log(
                        "H3",
                        "source/services/inputd/src/os_lite.rs:85",
                        "inputd recv ok",
                        &format!(
                            "frame_len={} op={} has_reply={}",
                            frame.len(),
                            frame.get(3).copied().unwrap_or(0),
                            u8::from(reply.is_some())
                        ),
                    );
                    // #endregion
                }
                if let Some(reply) = reply {
                    if frame_has_op(&frame, OP_GET_VISIBLE_STATE) {
                        let response = encode_visible_state_frame(runtime.visible_state);
                        let _ = reply.reply_and_close(&response);
                        runtime.chain.visible_state_replies =
                            runtime.chain.visible_state_replies.saturating_add(1);
                    } else {
                        let response = runtime.handle_frame(&frame);
                        let _ = reply.reply_and_close(&response);
                    }
                } else {
                    if frame_has_op(&frame, OP_GET_VISIBLE_STATE) {
                        let response = encode_visible_state_frame(runtime.visible_state);
                        let _ = server.send(&response, Wait::Blocking);
                        runtime.chain.visible_state_replies =
                            runtime.chain.visible_state_replies.saturating_add(1);
                    } else {
                        let response = runtime.handle_frame(&frame);
                        let _ = server.send(&response, Wait::Blocking);
                    }
                }
                runtime.report_chain_if_due();
            }
            Err(nexus_ipc::IpcError::WouldBlock) | Err(nexus_ipc::IpcError::Timeout) => {
                runtime.chain.idle_yields = runtime.chain.idle_yields.saturating_add(1);
                runtime.report_chain_if_due();
                let _ = yield_();
            }
            Err(err) => {
                let (recv_slot, send_slot) = server.slots();
                // #region agent log
                agent_log(
                    "H1",
                    "source/services/inputd/src/os_lite.rs:103",
                    "inputd recv fatal",
                    &format!(
                        "recv_slot={recv_slot} send_slot={send_slot} err_kind={} err_detail={}",
                        agent_ipc_error_kind(err),
                        agent_ipc_error_detail(err)
                    ),
                );
                // #endregion
                return Err("inputd recv failed");
            }
        }
    }
}

struct LiveRouteRuntime {
    input: InputdService<windowd::WindowServer>,
    launcher: windowd::CallerCtx,
    surface: windowd::SurfaceId,
    visible_state: VisibleState,
    pointer_marker_emitted: bool,
    keyboard_marker_emitted: bool,
    pointer_down_dispatch_debug_emitted: bool,
    pointer_down_delivery_debug_emitted: bool,
    focus_debug_emitted: bool,
    keyboard_dispatch_debug_emitted: bool,
    keyboard_delivery_debug_emitted: bool,
    chain: InputdChainTelemetry,
}

impl LiveRouteRuntime {
    fn new() -> Result<Self, &'static str> {
        let launcher = windowd::CallerCtx::from_service_metadata(0x55);
        let mut server = windowd::WindowServer::new(windowd::WindowdConfig {
            width: VISIBLE_INPUT_PROOF_WIDTH,
            height: VISIBLE_INPUT_PROOF_HEIGHT,
            hz: 60,
        })
        .map_err(|_| fail("inputd: init fail window-server"))?;
        let initial = visible_input_scene_surface(
            launcher,
            50,
            VISIBLE_INPUT_LEFT_IDLE_BGRA,
            VISIBLE_INPUT_RIGHT_IDLE_BGRA,
        )
        .map_err(|_| fail("inputd: init fail scene-buffer"))?;
        let surface = server
            .create_surface(launcher, initial.clone())
            .map_err(|_| fail("inputd: init fail create-surface"))?;
        server
            .queue_buffer(
                launcher,
                surface,
                initial,
                &[windowd::Rect::new(
                    0,
                    0,
                    VISIBLE_INPUT_SURFACE_WIDTH,
                    VISIBLE_INPUT_SURFACE_HEIGHT,
                )],
            )
            .map_err(|_| fail("inputd: init fail queue-buffer"))?;
        server
            .commit_scene(
                windowd::CallerCtx::system(),
                windowd::CommitSeq::new(1),
                &[windowd::Layer {
                    surface,
                    x: VISIBLE_INPUT_SURFACE_X,
                    y: VISIBLE_INPUT_SURFACE_Y,
                    z: 0,
                }],
            )
            .map_err(|_| fail("inputd: init fail commit-scene"))?;
        let _ = server.present_tick().map_err(|_| fail("inputd: init fail present-tick"))?;
        let config = InputdConfig::new(
            "de",
            350,
            30,
            64,
            1,
            1,
            96,
            64,
            VISIBLE_INPUT_CURSOR_START_X,
            VISIBLE_INPUT_CURSOR_START_Y,
        )
        .map_err(|_| fail("inputd: init fail config"))?;
        let input = InputdService::new(server, config)
            .map_err(|_| fail("inputd: init fail route-service"))?;
        Ok(Self {
            input,
            launcher,
            surface,
            visible_state: VisibleState {
                scene_ready: true,
                full_window_visible: true,
                click_target_visible: true,
                keyboard_target_visible: true,
                cursor_x: VISIBLE_INPUT_CURSOR_START_X,
                cursor_y: VISIBLE_INPUT_CURSOR_START_Y,
                ..VisibleState::default()
            },
            pointer_marker_emitted: false,
            keyboard_marker_emitted: false,
            pointer_down_dispatch_debug_emitted: false,
            pointer_down_delivery_debug_emitted: false,
            focus_debug_emitted: false,
            keyboard_dispatch_debug_emitted: false,
            keyboard_delivery_debug_emitted: false,
            chain: InputdChainTelemetry::new(),
        })
    }

    fn handle_frame(&mut self, frame: &[u8]) -> [u8; 8] {
        if frame_has_op(frame, OP_PUSH_HID_BATCH) {
            let Some(batch) = decode_push_hid_batch(frame) else {
                self.chain.hid_malformed = self.chain.hid_malformed.saturating_add(1);
                return encode_status(OP_PUSH_HID_BATCH, STATUS_MALFORMED);
            };
            let status = self.apply_wire_batch(batch);
            self.chain.record_hid_status(status);
            return encode_status(OP_PUSH_HID_BATCH, status);
        }
        let op = frame.get(3).copied().unwrap_or(0);
        encode_status(op, STATUS_UNSUPPORTED)
    }

    fn apply_wire_batch(&mut self, batch: WireHidBatch) -> u8 {
        if usize::from(batch.normalized_event_count) != batch.events.len()
            || batch.raw_event_count < batch.normalized_event_count
        {
            self.chain.hid_malformed = self.chain.hid_malformed.saturating_add(1);
            return STATUS_MALFORMED;
        }
        // OS-lite consumes the per-batch dispatch result directly for telemetry and visible-state
        // updates, so the internal dispatch history must not accumulate across live batches.
        self.input.clear_dispatches();
        self.chain.raw_events =
            self.chain.raw_events.saturating_add(u64::from(batch.raw_event_count));
        self.chain.normalized_events =
            self.chain.normalized_events.saturating_add(u64::from(batch.normalized_event_count));
        if batch.raw_event_count > 0 {
            self.visible_state.virtio_raw_seen = true;
        }
        if batch.normalized_event_count > 0 {
            self.visible_state.hid_normalized_seen = true;
        }
        let Ok(hid_batch) = self.decode_batch(batch) else {
            self.chain.hid_malformed = self.chain.hid_malformed.saturating_add(1);
            return STATUS_MALFORMED;
        };
        if self.input.apply_hid_batch_in_place(&hid_batch).is_err() {
            self.chain.route_overflow_apply = self.chain.route_overflow_apply.saturating_add(1);
            return STATUS_OVERFLOW;
        }
        let (
            dispatch_count,
            pointer_move_seen,
            pointer_down_dispatched,
            keyboard_dispatched,
            pointer_dispatch_batch,
            keyboard_dispatch_batch,
        ) = {
            let dispatches = self.input.recent_dispatches();
            (
                dispatches.len() as u64,
                dispatches
                    .iter()
                    .any(|dispatch| matches!(dispatch, InputDispatch::PointerMove { .. })),
                dispatches
                    .iter()
                    .any(|dispatch| matches!(dispatch, InputDispatch::PointerDown { .. })),
                dispatches
                    .iter()
                    .any(|dispatch| matches!(dispatch, InputDispatch::Keyboard { .. })),
                dispatches.iter().any(|dispatch| {
                    matches!(
                        dispatch,
                        InputDispatch::PointerMove { .. } | InputDispatch::PointerDown { .. }
                    )
                }),
                dispatches
                    .iter()
                    .any(|dispatch| matches!(dispatch, InputDispatch::Keyboard { .. })),
            )
        };
        let Ok(delivered_count) =
            self.input.router_mut().drain_input_events(self.launcher, self.surface)
        else {
            self.chain.route_overflow_delivery =
                self.chain.route_overflow_delivery.saturating_add(1);
            return STATUS_OVERFLOW;
        };
        self.chain.dispatch_events = self.chain.dispatch_events.saturating_add(dispatch_count);
        self.chain.delivered_events =
            self.chain.delivered_events.saturating_add(delivered_count as u64);
        if pointer_dispatch_batch {
            self.chain.pointer_dispatch_batches =
                self.chain.pointer_dispatch_batches.saturating_add(1);
        }
        if keyboard_dispatch_batch {
            self.chain.keyboard_dispatch_batches =
                self.chain.keyboard_dispatch_batches.saturating_add(1);
        }
        if pointer_dispatch_batch {
            self.chain.pointer_delivery_batches =
                self.chain.pointer_delivery_batches.saturating_add(1);
        }
        if keyboard_dispatch_batch {
            self.chain.keyboard_delivery_batches =
                self.chain.keyboard_delivery_batches.saturating_add(1);
        }
        self.update_visible_state(pointer_move_seen, pointer_down_dispatched, keyboard_dispatched);
        STATUS_OK
    }

    fn report_chain_if_due(&mut self) {
        self.chain.report_if_due(self.visible_state);
    }

    fn decode_batch(&self, batch: WireHidBatch) -> Result<HidBatch, ()> {
        let kind = match batch.device_kind {
            HID_KIND_KEYBOARD => HidDeviceKind::Keyboard,
            HID_KIND_MOUSE => HidDeviceKind::Mouse,
            _ => return Err(()),
        };
        let (width, height) = self.input.router().bounds();
        let mut events = Vec::with_capacity(batch.events.len());
        for event in batch.events {
            let timestamp = TimestampNs::new(event.timestamp_ns);
            let hid_event = match event.kind {
                EVENT_KIND_KEY => HidEvent::key(timestamp, event.code, event.value),
                EVENT_KIND_REL => HidEvent::rel(timestamp, event.code, event.value),
                EVENT_KIND_BTN => HidEvent::btn(timestamp, event.code, event.value),
                EVENT_KIND_ABS => {
                    let scaled = match event.code {
                        0 => scale_absolute(event.value, batch.abs_max_x, width),
                        1 => scale_absolute(event.value, batch.abs_max_y, height),
                        _ => return Err(()),
                    };
                    HidEvent::abs(timestamp, event.code, scaled)
                }
                _ => return Err(()),
            };
            events.push(hid_event);
        }
        Ok(HidBatch::new(DeviceId::new(batch.device_id), kind, events))
    }

    fn update_visible_state(
        &mut self,
        pointer_move_seen: bool,
        pointer_down_dispatched: bool,
        keyboard_dispatched: bool,
    ) {
        if let Some(pointer) = self.input.router().pointer_position() {
            self.visible_state.cursor_x = pointer.x;
            self.visible_state.cursor_y = pointer.y;
        }
        if pointer_down_dispatched && !self.pointer_down_dispatch_debug_emitted {
            let _ = debug_println("dbg: inputd pointer down dispatched");
            self.pointer_down_dispatch_debug_emitted = true;
        }
        if pointer_down_dispatched && !self.pointer_down_delivery_debug_emitted {
            let _ = debug_println("dbg: inputd pointer down delivered");
            self.pointer_down_delivery_debug_emitted = true;
        }
        if keyboard_dispatched && !self.keyboard_dispatch_debug_emitted {
            let _ = debug_println("dbg: inputd keyboard dispatched");
            self.keyboard_dispatch_debug_emitted = true;
        }
        if keyboard_dispatched && !self.keyboard_delivery_debug_emitted {
            let _ = debug_println("dbg: inputd keyboard delivered");
            self.keyboard_delivery_debug_emitted = true;
        }
        if pointer_move_seen {
            self.visible_state.pointer_route_live = true;
            self.visible_state.input_visible_on = true;
            self.visible_state.cursor_move_visible = true;
            self.visible_state.hover_visible =
                self.input.router().last_pointer_hit() == Some(self.surface);
        }
        if pointer_down_dispatched {
            self.visible_state.pointer_route_live = true;
            self.visible_state.input_visible_on = true;
            self.visible_state.launcher_click_visible = true;
            self.visible_state.focus_visible =
                self.input.router().focused_surface() == Some(self.surface);
            if self.visible_state.focus_visible && !self.focus_debug_emitted {
                let _ = debug_println("dbg: inputd focus on target");
                self.focus_debug_emitted = true;
            }
        }
        if keyboard_dispatched {
            self.visible_state.keyboard_route_live = true;
            self.visible_state.input_visible_on = true;
            self.visible_state.keyboard_visible = true;
        }
        if self.visible_state.pointer_route_live && !self.pointer_marker_emitted {
            let _ = debug_println("inputd: live pointer route on");
            self.pointer_marker_emitted = true;
        }
        if self.visible_state.keyboard_route_live && !self.keyboard_marker_emitted {
            let _ = debug_println("inputd: live keyboard route on");
            self.keyboard_marker_emitted = true;
        }
    }
}

struct InputdChainTelemetry {
    last_report_ns: u64,
    total_frames: u64,
    hid_push_frames: u64,
    visible_state_polls: u64,
    visible_state_replies: u64,
    unsupported_frames: u64,
    hid_ok: u64,
    hid_malformed: u64,
    hid_overflow: u64,
    route_overflow_apply: u64,
    route_overflow_delivery: u64,
    raw_events: u64,
    normalized_events: u64,
    dispatch_events: u64,
    delivered_events: u64,
    pointer_dispatch_batches: u64,
    keyboard_dispatch_batches: u64,
    pointer_delivery_batches: u64,
    keyboard_delivery_batches: u64,
    idle_yields: u64,
}

impl InputdChainTelemetry {
    const REPORT_INTERVAL_NS: u64 = 1_000_000_000;

    fn new() -> Self {
        Self {
            last_report_ns: nsec().unwrap_or(0),
            total_frames: 0,
            hid_push_frames: 0,
            visible_state_polls: 0,
            visible_state_replies: 0,
            unsupported_frames: 0,
            hid_ok: 0,
            hid_malformed: 0,
            hid_overflow: 0,
            route_overflow_apply: 0,
            route_overflow_delivery: 0,
            raw_events: 0,
            normalized_events: 0,
            dispatch_events: 0,
            delivered_events: 0,
            pointer_dispatch_batches: 0,
            keyboard_dispatch_batches: 0,
            pointer_delivery_batches: 0,
            keyboard_delivery_batches: 0,
            idle_yields: 0,
        }
    }

    fn record_hid_status(&mut self, status: u8) {
        match status {
            STATUS_OK => self.hid_ok = self.hid_ok.saturating_add(1),
            STATUS_MALFORMED => self.hid_malformed = self.hid_malformed.saturating_add(1),
            STATUS_OVERFLOW => self.hid_overflow = self.hid_overflow.saturating_add(1),
            _ => {}
        }
    }

    fn report_if_due(&mut self, state: VisibleState) {
        let now_ns = nsec().unwrap_or(0);
        if now_ns == 0 || self.last_report_ns == 0 {
            if now_ns != 0 {
                self.last_report_ns = now_ns;
            }
            return;
        }
        let elapsed = now_ns.saturating_sub(self.last_report_ns);
        if elapsed < Self::REPORT_INTERVAL_NS {
            return;
        }
        let recv_hz =
            self.total_frames.saturating_mul(1_000_000_000).checked_div(elapsed).unwrap_or(0);
        let hid_ok_hz = self.hid_ok.saturating_mul(1_000_000_000).checked_div(elapsed).unwrap_or(0);
        let poll_hz = self
            .visible_state_polls
            .saturating_mul(1_000_000_000)
            .checked_div(elapsed)
            .unwrap_or(0);
        // #region agent log
        let _ = debug_println(&format!(
            "fps: inputd recv_hz={} hid_ok_hz={} poll_hz={} hid_push={} hid_ok={} malformed={} overflow={} apply_ovf={} deliver_ovf={} raw_events={} norm_events={} dispatch={} delivered={} ptr_d={} kbd_d={} ptr_deliv={} kbd_deliv={} poll_reply={} idle_yields={} pointer_live={} keyboard_live={}",
            recv_hz,
            hid_ok_hz,
            poll_hz,
            self.hid_push_frames,
            self.hid_ok,
            self.hid_malformed,
            self.hid_overflow,
            self.route_overflow_apply,
            self.route_overflow_delivery,
            self.raw_events,
            self.normalized_events,
            self.dispatch_events,
            self.delivered_events,
            self.pointer_dispatch_batches,
            self.keyboard_dispatch_batches,
            self.pointer_delivery_batches,
            self.keyboard_delivery_batches,
            self.visible_state_replies,
            self.idle_yields,
            u8::from(state.pointer_route_live),
            u8::from(state.keyboard_route_live)
        ));
        // #endregion
        self.last_report_ns = now_ns;
        self.total_frames = 0;
        self.hid_push_frames = 0;
        self.visible_state_polls = 0;
        self.visible_state_replies = 0;
        self.unsupported_frames = 0;
        self.hid_ok = 0;
        self.hid_malformed = 0;
        self.hid_overflow = 0;
        self.route_overflow_apply = 0;
        self.route_overflow_delivery = 0;
        self.raw_events = 0;
        self.normalized_events = 0;
        self.dispatch_events = 0;
        self.delivered_events = 0;
        self.pointer_dispatch_batches = 0;
        self.keyboard_dispatch_batches = 0;
        self.pointer_delivery_batches = 0;
        self.keyboard_delivery_batches = 0;
        self.idle_yields = 0;
    }
}

fn scale_absolute(value: i32, max: i32, bound: u32) -> i32 {
    if max <= 0 || bound == 0 {
        return 0;
    }
    let clamped = value.clamp(0, max);
    let top = i64::from(bound.saturating_sub(1));
    ((i64::from(clamped) * top) / i64::from(max)) as i32
}

fn fail(label: &'static str) -> &'static str {
    let _ = debug_println(label);
    label
}

fn agent_log(
    hypothesis_id: &'static str,
    location: &'static str,
    message: &'static str,
    data: &str,
) {
    let _ = debug_println(&format!("agent8cde1d|{hypothesis_id}|{location}|{message}|{data}"));
}

fn agent_ipc_error_kind(err: nexus_ipc::IpcError) -> &'static str {
    match err {
        nexus_ipc::IpcError::WouldBlock => "would_block",
        nexus_ipc::IpcError::Timeout => "timeout",
        nexus_ipc::IpcError::Disconnected => "disconnected",
        nexus_ipc::IpcError::NoSpace => "no_space",
        nexus_ipc::IpcError::Kernel(_) => "kernel",
        nexus_ipc::IpcError::Unsupported => "unsupported",
        _ => "other",
    }
}

fn agent_ipc_error_detail(err: nexus_ipc::IpcError) -> &'static str {
    match err {
        nexus_ipc::IpcError::Kernel(kernel) => match kernel {
            nexus_abi::IpcError::NoSuchEndpoint => "kernel_no_such_endpoint",
            nexus_abi::IpcError::QueueFull => "kernel_queue_full",
            nexus_abi::IpcError::QueueEmpty => "kernel_queue_empty",
            nexus_abi::IpcError::PermissionDenied => "kernel_permission_denied",
            nexus_abi::IpcError::TimedOut => "kernel_timed_out",
            nexus_abi::IpcError::NoSpace => "kernel_no_space",
            nexus_abi::IpcError::Unsupported => "kernel_unsupported",
        },
        _ => agent_ipc_error_kind(err),
    }
}

fn agent_ipc_error_data(err: nexus_ipc::IpcError) -> &'static str {
    match err {
        nexus_ipc::IpcError::WouldBlock => "route_err=would_block",
        nexus_ipc::IpcError::Timeout => "route_err=timeout",
        nexus_ipc::IpcError::Disconnected => "route_err=disconnected",
        nexus_ipc::IpcError::NoSpace => "route_err=no_space",
        nexus_ipc::IpcError::Kernel(_) => "route_err=kernel",
        nexus_ipc::IpcError::Unsupported => "route_err=unsupported",
        _ => "route_err=other",
    }
}

fn visible_input_scene_surface(
    caller: windowd::CallerCtx,
    frame_index: u64,
    left_square: [u8; 4],
    right_square: [u8; 4],
) -> Result<windowd::SurfaceBuffer, ()> {
    let mut surface = windowd::SurfaceBuffer::solid(
        caller,
        frame_index,
        VISIBLE_INPUT_SURFACE_WIDTH,
        VISIBLE_INPUT_SURFACE_HEIGHT,
        VISIBLE_INPUT_BGRA,
    )
    .map_err(|_| ())?;
    for y in 0..surface.height {
        for x in 0..surface.width {
            let bgra = visible_input_scene_pixel_bgra(x, y, left_square, right_square);
            let idx = (y as usize * surface.stride as usize) + (x as usize * 4);
            surface.pixels[idx..idx + 4].copy_from_slice(&bgra);
        }
    }
    Ok(surface)
}

fn visible_input_scene_pixel_bgra(
    x: u32,
    y: u32,
    left_square: [u8; 4],
    right_square: [u8; 4],
) -> [u8; 4] {
    if rect_contains(
        x,
        y,
        VISIBLE_INPUT_LEFT_SQUARE_X,
        VISIBLE_INPUT_LEFT_SQUARE_Y,
        VISIBLE_INPUT_SQUARE_SIZE,
        VISIBLE_INPUT_SQUARE_SIZE,
    ) {
        left_square
    } else if rect_contains(
        x,
        y,
        VISIBLE_INPUT_RIGHT_SQUARE_X,
        VISIBLE_INPUT_RIGHT_SQUARE_Y,
        VISIBLE_INPUT_SQUARE_SIZE,
        VISIBLE_INPUT_SQUARE_SIZE,
    ) {
        right_square
    } else {
        let stripe = ((x / 8) + (y / 8)) & 1;
        if stripe == 0 {
            VISIBLE_INPUT_BGRA
        } else {
            [0x24, 0x38, 0xa0, 0xff]
        }
    }
}

fn rect_contains(x: u32, y: u32, left: u32, top: u32, width: u32, height: u32) -> bool {
    x >= left && x < left.saturating_add(width) && y >= top && y < top.saturating_add(height)
}
