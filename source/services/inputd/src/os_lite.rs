// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: OS-lite `inputd` live route backend for RFC-0054.
//! OWNERS: @runtime @ui
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `cargo test -p inputd -- --nocapture`
//! ADR: docs/adr/0029-input-v1-host-core-architecture.md

extern crate alloc;

use alloc::format;

use hidrawd::PointerSource;
use input_live_protocol::{
    decode_push_hid_batch, encode_status, encode_update_visible_state, encode_visible_state_frame,
    frame_has_op, VisibleState, WireHidBatch, OP_GET_VISIBLE_STATE, OP_PUSH_HID_BATCH,
    STATUS_MALFORMED, STATUS_OK, STATUS_OVERFLOW, STATUS_UNSUPPORTED,
};
use keymaps::{KeyAction, KeyOutput};
use nexus_abi::{debug_println, nsec, yield_};
use nexus_ipc::{Client as _, KernelClient, KernelServer, Server as _, Wait};

use crate::route::NormalizeRouter;
use crate::{
    decode_wire_batch, live_push::should_push_visible_state, visible_display_space,
    visible_display_start_position, InputDispatch, InputdConfig, InputdService, WireBatchReject,
    LIVE_POINTER_DENOMINATOR, LIVE_POINTER_MAX_OUTPUT, LIVE_POINTER_NUMERATOR,
    LIVE_POINTER_THRESHOLD,
};

const WHEEL_INDICATOR_PULSE_NS: u64 = 120_000_000;
const ROUTE_BIND_RETRIES: usize = 256;

pub fn service_main_loop() -> Result<(), &'static str> {
    // Caps are pre-granted by init before resume — no yield needed.

    let mut runtime = match LiveRouteRuntime::new() {
        Ok(rt) => rt,
        Err(e) => {
            let _ = debug_println(e);
            let _ = debug_println("inputd: init fail runtime");
            return Err(e);
        }
    };
    let server = match bind_server() {
        Ok(server) => server,
        Err(err) => {
            let _ = debug_println(match err {
                nexus_ipc::IpcError::WouldBlock => "inputd: route probe would-block",
                nexus_ipc::IpcError::Timeout => "inputd: route probe miss",
                nexus_ipc::IpcError::Disconnected => "inputd: route probe disconnected",
                nexus_ipc::IpcError::NoSpace => "inputd: route probe no-space",
                nexus_ipc::IpcError::Kernel(_) => "inputd: route probe kernel",
                nexus_ipc::IpcError::Unsupported => "inputd: route probe unsupported",
                _ => "inputd: route probe other",
            });
            let _ = debug_println("inputd: route fallback");
            let _ = debug_println("inputd: fallback slots 3/4");
            let server = KernelServer::new_with_slots(3, 4)
                .map_err(|_| fail("inputd: init fail kernel-server"))?;
            server
        }
    };
    debug_println("inputd: ready").map_err(|_| "inputd ready log failed")?;
    debug_println("inputd: keymap=de").map_err(|_| "inputd keymap log failed")?;
    debug_println("inputd: os service payload ready").map_err(|_| "inputd payload log failed")?;
    loop {
        match server.recv_request_with_meta(Wait::Timeout(core::time::Duration::from_millis(16))) {
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
                if let Some(reply) = reply {
                    if frame_has_op(&frame, OP_GET_VISIBLE_STATE) {
                        let response = encode_visible_state_frame(runtime.visible_state_snapshot());
                        let _ = reply.reply_and_close(&response);
                        runtime.chain.visible_state_replies =
                            runtime.chain.visible_state_replies.saturating_add(1);
                    } else {
                        let response = runtime.handle_frame(&frame);
                        let _ = reply.reply_and_close(&response);
                    }
                } else {
                    if frame_has_op(&frame, OP_GET_VISIBLE_STATE) {
                        let response = encode_visible_state_frame(runtime.visible_state_snapshot());
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
                runtime.expire_transient_input_state();
                runtime.chain.idle_yields = runtime.chain.idle_yields.saturating_add(1);
                runtime.report_chain_if_due();
                let _ = yield_();
            }
            Err(_err) => {
                return Err("inputd recv failed");
            }
        }
    }
}

fn bind_server() -> core::result::Result<KernelServer, nexus_ipc::IpcError> {
    let mut last_err = nexus_ipc::IpcError::Unsupported;
    for _ in 0..ROUTE_BIND_RETRIES {
        match KernelServer::new_for("inputd") {
            Ok(server) => return Ok(server),
            Err(err) => last_err = err,
        }
        let _ = yield_();
    }
    Err(last_err)
}

struct LiveRouteRuntime {
    input: InputdService<NormalizeRouter>,
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
    hid_batch_recv_debug_emitted: bool,
    chain_normalize_ok_emitted: bool,
    chain_normalize_fail_emitted: bool,
    absolute_source_debug_emitted: bool,
    relative_blocked_debug_emitted: bool,
    windowd_push_ok_emitted: bool,
    windowd_route_fallback_emitted: bool,
    windowd_push_fail_emitted: bool,
    wheel_indicator_direction: WheelIndicatorDirection,
    wheel_indicator_deadline_ns: u64,
    windowd_client: Option<KernelClient>,
    last_windowd_push_state: Option<VisibleState>,
    last_windowd_push_ns: u64,
    chain: InputdChainTelemetry,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WheelIndicatorDirection {
    None,
    Up,
    Down,
}

impl LiveRouteRuntime {
    fn new() -> Result<Self, &'static str> {
        let launcher = windowd::CallerCtx::from_service_metadata(0x55);
        // Production-grade input pipeline: inputd owns NO
        // window server and does NO hit-testing. A pure NormalizeRouter handles
        // pointer transform + coalescing; windowd (the compositor, interaction.rs
        // SSOT since A1) resolves all hover/click/focus against its own rendered
        // geometry. The old embedded windowd::WindowServer + its never-displayed
        // "visible input proof" surface are gone — that was a legacy dual.
        let display_start = visible_display_start_position()
            .map_err(|_| fail("inputd: init fail pointer-transform"))?;
        let display_space =
            visible_display_space().map_err(|_| fail("inputd: init fail pointer-transform"))?;
        // Unrouted surface 0: inputd forwards normalized input; windowd routes it.
        let surface = windowd::SurfaceId::new(0);
        let router = NormalizeRouter::new(display_space.width(), display_space.height(), 2);
        let config = InputdConfig::new(
            "de",
            350,
            30,
            LIVE_POINTER_THRESHOLD,
            LIVE_POINTER_NUMERATOR,
            LIVE_POINTER_DENOMINATOR,
            LIVE_POINTER_MAX_OUTPUT,
            64,
            display_start.x,
            display_start.y,
        )
        .and_then(|config| config.with_display_space(display_space.width(), display_space.height()))
        .map_err(|_| fail("inputd: init fail config"))?;
        let input = InputdService::new(router, config)
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
                cursor_x: display_start.x,
                cursor_y: display_start.y,
                cursor_move_visible: true,
                ..VisibleState::default()
            },
            pointer_marker_emitted: false,
            keyboard_marker_emitted: false,
            pointer_down_dispatch_debug_emitted: false,
            pointer_down_delivery_debug_emitted: false,
            focus_debug_emitted: false,
            keyboard_dispatch_debug_emitted: false,
            keyboard_delivery_debug_emitted: false,
            hid_batch_recv_debug_emitted: false,
            chain_normalize_ok_emitted: false,
            chain_normalize_fail_emitted: false,
            absolute_source_debug_emitted: false,
            relative_blocked_debug_emitted: false,
            windowd_push_ok_emitted: false,
            windowd_route_fallback_emitted: false,
            windowd_push_fail_emitted: false,
            wheel_indicator_direction: WheelIndicatorDirection::None,
            wheel_indicator_deadline_ns: 0,
            windowd_client: KernelClient::new_with_slots(5, 6).ok(),
            last_windowd_push_state: None,
            last_windowd_push_ns: 0,
            chain: InputdChainTelemetry::new(),
        })
    }

    fn visible_state_snapshot(&mut self) -> VisibleState {
        self.sync_wheel_indicator(nsec().unwrap_or(0));
        self.visible_state
    }

    fn handle_frame(&mut self, frame: &[u8]) -> [u8; 8] {
        if frame_has_op(frame, OP_PUSH_HID_BATCH) {
            if !self.hid_batch_recv_debug_emitted {
                let _ = debug_println("dbg: inputd hid batch recv");
                // Input-chain hop I3: a wire batch arrived from hidrawd.
                let _ = debug_println("inputd: chain I3 wire recv from hidrawd");
                self.hid_batch_recv_debug_emitted = true;
            }
            let Some(batch) = decode_push_hid_batch(frame) else {
                self.chain.frame_decode_malformed =
                    self.chain.frame_decode_malformed.saturating_add(1);
                self.chain.hid_malformed = self.chain.hid_malformed.saturating_add(1);
                // Input-chain hop I4 fail: the wire batch could not be decoded.
                if !self.chain_normalize_fail_emitted {
                    let _ = debug_println("inputd: chain I4 normalize FAIL (malformed wire batch)");
                    self.chain_normalize_fail_emitted = true;
                }
                return encode_status(OP_PUSH_HID_BATCH, STATUS_MALFORMED);
            };
            // Input-chain hop I4: the wire batch decoded into normalized events.
            if !self.chain_normalize_ok_emitted {
                let _ = debug_println("inputd: chain I4 normalized");
                self.chain_normalize_ok_emitted = true;
            }
            let status = self.apply_wire_batch(batch);
            self.chain.record_hid_status(status);
            return encode_status(OP_PUSH_HID_BATCH, status);
        }
        let op = frame.get(3).copied().unwrap_or(0);
        encode_status(op, STATUS_UNSUPPORTED)
    }

    fn apply_wire_batch(&mut self, batch: WireHidBatch) -> u8 {
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
        let batch_pointer_source = batch.pointer_source;
        let batch_normalized_event_count = batch.normalized_event_count;
        let hid_batch = match decode_wire_batch(batch, self.input.pointer_transform()) {
            Ok(batch) => batch,
            Err(reject) => {
                self.chain.record_wire_reject(reject);
                let _ = debug_println(&format!("inputd: reject {}", reject.label()));
                return reject.status();
            }
        };
        let previous_source = self.input.active_pointer_source();
        if self.input.apply_hid_batch_in_place(&hid_batch).is_err() {
            self.chain.route_overflow_apply = self.chain.route_overflow_apply.saturating_add(1);
            return STATUS_OVERFLOW;
        }
        let active_source = self.input.active_pointer_source();
        let (
            dispatch_count,
            pointer_move_seen,
            pointer_down_dispatched,
            pointer_wheel_delta,
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
                    .filter_map(|dispatch| match dispatch {
                        InputDispatch::PointerWheel { delta_y } => Some(*delta_y),
                        _ => None,
                    })
                    .sum(),
                dispatches
                    .iter()
                    .any(|dispatch| matches!(dispatch, InputDispatch::Keyboard { .. })),
                dispatches.iter().any(|dispatch| {
                    matches!(
                        dispatch,
                        InputDispatch::PointerMove { .. }
                            | InputDispatch::PointerDown { .. }
                            | InputDispatch::PointerWheel { .. }
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
        self.apply_visible_text_input();
        if !self.absolute_source_debug_emitted
            && matches!(
                active_source,
                Some(PointerSource::TabletAbsolute | PointerSource::TouchAbsolute)
            )
            && active_source != previous_source
        {
            let _ = debug_println("dbg: inputd active source absolute");
            self.absolute_source_debug_emitted = true;
        }
        if !self.relative_blocked_debug_emitted
            && batch_pointer_source == input_live_protocol::POINTER_SOURCE_MOUSE_RELATIVE
            && batch_normalized_event_count > 0
            && dispatch_count == 0
            && matches!(
                active_source,
                Some(PointerSource::TabletAbsolute | PointerSource::TouchAbsolute)
            )
        {
            let _ = debug_println("dbg: inputd relative blocked by absolute source");
            self.relative_blocked_debug_emitted = true;
        }
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
        self.update_visible_state(
            pointer_move_seen,
            pointer_down_dispatched,
            pointer_wheel_delta,
            keyboard_dispatched,
            active_source,
        );
        STATUS_OK
    }

    fn apply_visible_text_input(&mut self) {
        if self.input.router().focused_surface() != Some(self.surface) {
            return;
        }
        for dispatch in self.input.recent_dispatches() {
            let InputDispatch::Keyboard { output, .. } = dispatch else {
                continue;
            };
            match output {
                KeyOutput::Text(ch) => {
                    let _ = self.visible_state.push_text_char(*ch);
                }
                KeyOutput::Action(KeyAction::Backspace) => {
                    let _ = self.visible_state.pop_text_char();
                }
                _ => {}
            }
        }
    }

    fn report_chain_if_due(&mut self) {
        self.chain.report_if_due(self.visible_state);
    }

    fn update_visible_state(
        &mut self,
        pointer_move_seen: bool,
        pointer_down_dispatched: bool,
        pointer_wheel_delta: i32,
        keyboard_dispatched: bool,
        // Pointer source is no longer needed here — absolute moves used to force an
        // immediate push, but moves now always go through the 120 Hz budget.
        _active_source: Option<PointerSource>,
    ) {
        let now_ns = nsec().unwrap_or(0);
        // Pure input normalization: inputd ships the display-space pointer plus
        // raw button/wheel/key facts. It does NOT hit-test — windowd owns all UI
        // intent (hover/click/scroll/focus) against its own rendered geometry
        // (the compositor model). This is why the legacy 64×48 "route" hit-test
        // is gone: a downscaled proof-space pointer never matched the real
        // 1280×800 control rects.
        let display_pointer = self.input.display_pointer_position();
        let pointer_held = self.input.primary_pointer_held();
        let keyboard_held = self.input.held_non_modifier_key_count() > 0;
        let previous_launcher_click = self.visible_state.launcher_click_visible;
        let previous_focus_visible = self.visible_state.focus_visible;
        self.visible_state.cursor_x = display_pointer.x;
        self.visible_state.cursor_y = display_pointer.y;
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
        }
        self.visible_state.focus_visible = false;
        if pointer_down_dispatched {
            self.visible_state.pointer_route_live = true;
            self.visible_state.input_visible_on = true;
            self.visible_state.focus_visible =
                self.input.router().focused_surface() == Some(self.surface);
            if self.visible_state.focus_visible && !self.focus_debug_emitted {
                let _ = debug_println("dbg: inputd focus on target");
                self.focus_debug_emitted = true;
            }
        }
        // Carry the real signed wheel magnitude (0 when no wheel motion this
        // update) so windowd scrolls by the actual notch count, not one quantized
        // step. The pulse booleans below remain a latched direction indicator.
        self.visible_state.wheel_delta_y = pointer_wheel_delta;
        if pointer_wheel_delta != 0 {
            self.visible_state.pointer_route_live = true;
            self.visible_state.input_visible_on = true;
            self.note_wheel_indicator(pointer_wheel_delta, now_ns);
        }
        // Raw primary-button level. windowd detects the click edge and resolves
        // it (sidebar toggle / focus) against the rendered geometry. hover_visible
        // and sidebar_open_visible are no longer produced here — they are
        // windowd-owned derived state.
        self.visible_state.launcher_click_visible = pointer_held;
        if keyboard_dispatched {
            self.visible_state.keyboard_route_live = true;
            self.visible_state.input_visible_on = true;
        }
        if self.visible_state.keyboard_route_live {
            self.visible_state.keyboard_visible = keyboard_held;
        }
        if self.visible_state.pointer_route_live && !self.pointer_marker_emitted {
            let _ = debug_println("inputd: live pointer route on");
            self.pointer_marker_emitted = true;
        }
        if self.visible_state.keyboard_route_live && !self.keyboard_marker_emitted {
            let _ = debug_println("inputd: live keyboard route on");
            self.keyboard_marker_emitted = true;
        }
        self.sync_wheel_indicator(now_ns);
        // Push immediately ONLY on discrete edges (button/key/focus) + wheel, so
        // windowd sees clean click edges and low-latency scroll. Pointer MOVES —
        // including absolute (tablet/touch) — go through the 120 Hz push budget
        // (`should_push_visible_state`), NOT immediately: an absolute pointer can
        // emit ~800 moves/s, and pushing each one floods windowd. Throttling moves
        // to display rate is what every OS does (the cursor still tracks at 120 Hz),
        // and windowd coalesces frame-aligned regardless.
        let button_changed = previous_launcher_click != self.visible_state.launcher_click_visible;
        let focus_changed = previous_focus_visible != self.visible_state.focus_visible;
        let immediate_push = pointer_down_dispatched
            || pointer_wheel_delta != 0
            || keyboard_dispatched
            || button_changed
            || focus_changed;
        self.push_visible_state_to_windowd(now_ns, immediate_push);
    }

    fn note_wheel_indicator(&mut self, delta_y: i32, now_ns: u64) {
        self.wheel_indicator_direction = if delta_y > 0 {
            WheelIndicatorDirection::Up
        } else if delta_y < 0 {
            WheelIndicatorDirection::Down
        } else {
            WheelIndicatorDirection::None
        };
        self.wheel_indicator_deadline_ns = now_ns.saturating_add(WHEEL_INDICATOR_PULSE_NS);
    }

    fn sync_wheel_indicator(&mut self, now_ns: u64) {
        let active = now_ns <= self.wheel_indicator_deadline_ns;
        self.visible_state.wheel_up_visible =
            active && self.wheel_indicator_direction == WheelIndicatorDirection::Up;
        self.visible_state.wheel_down_visible =
            active && self.wheel_indicator_direction == WheelIndicatorDirection::Down;
        if !active {
            self.wheel_indicator_direction = WheelIndicatorDirection::None;
        }
    }

    fn expire_transient_input_state(&mut self) {
        let now_ns = nsec().unwrap_or(0);
        let old_up = self.visible_state.wheel_up_visible;
        let old_down = self.visible_state.wheel_down_visible;
        self.sync_wheel_indicator(now_ns);
        let immediate_push = old_up != self.visible_state.wheel_up_visible
            || old_down != self.visible_state.wheel_down_visible;
        if should_push_visible_state(
            self.last_windowd_push_state,
            self.visible_state,
            self.last_windowd_push_ns,
            now_ns,
            immediate_push,
        ) {
            self.push_visible_state_to_windowd(now_ns, immediate_push);
        }
    }

    fn push_visible_state_to_windowd(&mut self, now_ns: u64, immediate: bool) {
        if !should_push_visible_state(
            self.last_windowd_push_state,
            self.visible_state,
            self.last_windowd_push_ns,
            now_ns,
            immediate,
        ) {
            return;
        }
        if self.windowd_client.is_none() {
            self.windowd_client = KernelClient::new_for("windowd").ok();
            if self.windowd_client.is_none() && !self.windowd_route_fallback_emitted {
                let _ = debug_println("inputd: windowd route unavailable");
                self.windowd_route_fallback_emitted = true;
                // Fall back to priority-wired slots from init (5=send, 6=recv).
                self.windowd_client = KernelClient::new_with_slots(5, 6).ok();
                if self.windowd_client.is_some() {
                    let _ = debug_println("inputd: windowd route fallback slots 5/6");
                }
            }
        }
        let Some(client) = &self.windowd_client else {
            return;
        };
        let frame = encode_update_visible_state(self.visible_state);
        match client.send(&frame, Wait::Timeout(core::time::Duration::from_millis(2))) {
            Ok(()) => {
                self.last_windowd_push_state = Some(self.visible_state);
                self.last_windowd_push_ns = now_ns;
                if !self.windowd_push_ok_emitted {
                    let _ = debug_println("inputd: windowd visible-state pushed");
                    // Input-chain hop I5: normalized state delivered to windowd.
                    let _ = debug_println("inputd: chain I5 delivered to windowd");
                    self.windowd_push_ok_emitted = true;
                }
            }
            Err(nexus_ipc::IpcError::WouldBlock)
            | Err(nexus_ipc::IpcError::Timeout)
            | Err(nexus_ipc::IpcError::NoSpace) => {
                // Backpressure only: keep route/client and retry next tick.
            }
            Err(_) => {
                if !self.windowd_push_fail_emitted {
                    let _ = debug_println("inputd: windowd visible-state push fail");
                    // Input-chain hop I5 fail: windowd unreachable (route dropped).
                    let _ = debug_println("inputd: chain I5 deliver FAIL (windowd route)");
                    self.windowd_push_fail_emitted = true;
                }
                self.windowd_client = None;
            }
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
    hid_unsupported: u64,
    hid_overflow: u64,
    frame_decode_malformed: u64,
    wire_count_rejects: u64,
    wire_device_kind_rejects: u64,
    wire_pointer_source_rejects: u64,
    wire_event_kind_rejects: u64,
    wire_source_mode_rejects: u64,
    wire_abs_calibration_rejects: u64,
    wire_abs_axis_rejects: u64,
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
            hid_unsupported: 0,
            hid_overflow: 0,
            frame_decode_malformed: 0,
            wire_count_rejects: 0,
            wire_device_kind_rejects: 0,
            wire_pointer_source_rejects: 0,
            wire_event_kind_rejects: 0,
            wire_source_mode_rejects: 0,
            wire_abs_calibration_rejects: 0,
            wire_abs_axis_rejects: 0,
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
            STATUS_UNSUPPORTED => self.hid_unsupported = self.hid_unsupported.saturating_add(1),
            STATUS_OVERFLOW => self.hid_overflow = self.hid_overflow.saturating_add(1),
            _ => {}
        }
    }

    fn record_wire_reject(&mut self, reject: WireBatchReject) {
        match reject {
            WireBatchReject::CountMismatch | WireBatchReject::RawCountUnderflow => {
                self.wire_count_rejects = self.wire_count_rejects.saturating_add(1)
            }
            WireBatchReject::UnknownDeviceKind(_) => {
                self.wire_device_kind_rejects = self.wire_device_kind_rejects.saturating_add(1)
            }
            WireBatchReject::KeyboardPointerSource(_)
            | WireBatchReject::MissingPointerSource
            | WireBatchReject::UnknownPointerSource(_) => {
                self.wire_pointer_source_rejects =
                    self.wire_pointer_source_rejects.saturating_add(1)
            }
            WireBatchReject::KeyboardEventKind(_)
            | WireBatchReject::PointerKeyEvent
            | WireBatchReject::UnknownEventKind(_) => {
                self.wire_event_kind_rejects = self.wire_event_kind_rejects.saturating_add(1)
            }
            WireBatchReject::RelativeOnAbsoluteSource(_)
            | WireBatchReject::AbsoluteOnRelativeSource(_) => {
                self.wire_source_mode_rejects = self.wire_source_mode_rejects.saturating_add(1)
            }
            WireBatchReject::InvalidAbsoluteCalibration(_) => {
                self.wire_abs_calibration_rejects =
                    self.wire_abs_calibration_rejects.saturating_add(1)
            }
            WireBatchReject::InvalidAbsoluteAxis(_) => {
                self.wire_abs_axis_rejects = self.wire_abs_axis_rejects.saturating_add(1)
            }
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
            "fps: inputd recv_hz={} hid_ok_hz={} poll_hz={} hid_push={} hid_ok={} malformed={} hid_unsupported={} overflow={} frame_malformed={} wire_count={} wire_kind={} wire_source={} wire_event={} wire_mode={} abs_cal={} abs_axis={} apply_ovf={} deliver_ovf={} raw_events={} norm_events={} dispatch={} delivered={} ptr_d={} kbd_d={} ptr_deliv={} kbd_deliv={} poll_reply={} idle_yields={} pointer_live={} keyboard_live={}",
            recv_hz,
            hid_ok_hz,
            poll_hz,
            self.hid_push_frames,
            self.hid_ok,
            self.hid_malformed,
            self.hid_unsupported,
            self.hid_overflow,
            self.frame_decode_malformed,
            self.wire_count_rejects,
            self.wire_device_kind_rejects,
            self.wire_pointer_source_rejects,
            self.wire_event_kind_rejects,
            self.wire_source_mode_rejects,
            self.wire_abs_calibration_rejects,
            self.wire_abs_axis_rejects,
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
        self.hid_unsupported = 0;
        self.hid_overflow = 0;
        self.frame_decode_malformed = 0;
        self.wire_count_rejects = 0;
        self.wire_device_kind_rejects = 0;
        self.wire_pointer_source_rejects = 0;
        self.wire_event_kind_rejects = 0;
        self.wire_source_mode_rejects = 0;
        self.wire_abs_calibration_rejects = 0;
        self.wire_abs_axis_rejects = 0;
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

fn fail(label: &'static str) -> &'static str {
    let _ = debug_println(label);
    label
}
