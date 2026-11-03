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
use alloc::vec::Vec;
use hid::HidEvent;
use nexus_service_topology::slots::inputd as topo;

use hidrawd::PointerSource;
use input_live_protocol::{
    decode_push_hid_batch_reusing, encode_status, encode_visible_state_frame, frame_has_op,
    motion_push::encode_update_visible_state_motion, VisibleState, WireHidBatch,
    OP_GET_VISIBLE_STATE, OP_PUSH_HID_BATCH, STATUS_MALFORMED, STATUS_OK, STATUS_OVERFLOW,
    STATUS_UNSUPPORTED,
};
use nexus_abi::{debug_println, debug_trace, nsec};
use nexus_ipc::{Client as _, KernelClient, KernelServer, Server as _, Wait};

use crate::route::NormalizeRouter;
use crate::{
    decode_wire_batch_reusing,
    live_push::{should_push_visible_state, POINTER_PUSH_INTERVAL_NS},
    visible_display_space, visible_display_start_position, InputDispatch, InputdConfig,
    InputdService, LIVE_POINTER_DENOMINATOR, LIVE_POINTER_MAX_OUTPUT, LIVE_POINTER_NUMERATOR,
    LIVE_POINTER_THRESHOLD,
};

// TASK-0324 P7-d: the waitset + pacing-timer side of the loop (child module: it reaches the
// runtime's private state without widening any visibility).
mod display_space;
/// RFC-0113: the key channel to windowd, the stream checks, the layout relay.
mod keys;
mod wait;
use wait::{build_waitset, serve_request};

const WHEEL_INDICATOR_PULSE_NS: u64 = 120_000_000;
/// Requests served per pass before the loop drains its other waitset members.
const IPC_BATCH_LIMIT: usize = 64;

use crate::chain_stats::InputdChainTelemetry;

/// Wheel indicator + HID-rate telemetry (a pure move, structure ratchet).
mod indicators;
mod state_push;

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
            let _ = nexus_abi::trace_line("inputd: route fallback");
            let _ = nexus_abi::trace_line("inputd: fallback slots 3/4");
            let server = KernelServer::new_with_slots(topo::SERVER.recv, topo::SERVER.send)
                .map_err(|_| fail("inputd: init fail kernel-server"))?;
            server
        }
    };
    nexus_service_entry::ready("inputd: ready").map_err(|_| "inputd ready log failed")?;
    debug_println("inputd: keymap=de").map_err(|_| "inputd keymap log failed")?;
    debug_println("inputd: os service payload ready").map_err(|_| "inputd payload log failed")?;
    // RFC-0068: ready reached — emit the folded `inputd N/N` verdict (interactive only).
    nexus_abi::service_verdict_flush("inputd");
    // TASK-0324 P7-d: ONE waitset over the server endpoint, the settings push channel and the
    // timer-notify endpoint. The two clock-bound facts inputd has — the wheel indicator's
    // expiry and the throttled pointer push — are paced by a kernel ONE-SHOT timer armed at
    // the exact deadline: no recv timeout, no 16 ms idle tick; nothing pending = zero wakes.
    let waitset = build_waitset(&server);
    let mut timer = nexus_ipc::timer::NotifyTimer::bind(topo::TIMER).ok();
    if waitset.is_none() || timer.is_none() {
        let _ = debug_println("inputd: FAIL waitset/timer (blocking on the server endpoint alone)");
    }
    runtime.subscribe_settings_watch();
    // ONE loop buffer: an allocating recv killed inputd (TASK-0054C P2-g).
    let mut recv_frame = [0u8; input_live_protocol::MAX_HID_BATCH_FRAME_LEN];
    loop {
        // 1. Requests, in a bounded batch (a blocking one when the waitset is unavailable).
        let wait = if waitset.is_some() { Wait::NonBlocking } else { Wait::Blocking };
        for _ in 0..IPC_BATCH_LIMIT {
            match server.recv_request_with_meta_into(wait, &mut recv_frame) {
                Ok((n, _s, r)) => serve_request(&mut runtime, &server, &recv_frame[..n], r),
                Err(nexus_ipc::IpcError::WouldBlock) | Err(nexus_ipc::IpcError::Timeout) => break,
                Err(_) => return Err("inputd recv failed"),
            }
            if waitset.is_none() {
                break;
            }
        }
        // 2. Pushes and timer notifications: waitset members, drained, never polled.
        runtime.drain_settings_pushes();
        if let Some(t) = timer.as_mut() {
            t.drain();
        }
        runtime.tick_key_repeat();
        // 3. The clock-bound state, then the timer for whatever is still pending.
        runtime.expire_transient_input_state();
        runtime.report_chain_if_due();
        if let Some(t) = timer.as_mut() {
            t.arm_at(runtime.next_pacing_deadline_ns().unwrap_or(0));
        }
        // 4. WAIT — no clock.
        if let Some(ws) = waitset {
            let _ = nexus_abi::waitset_wait(ws, 0);
        }
    }
}

/// inputd's own server endpoint: the declared slots (TASK-0324 P4b), pinned by init before
/// the task is resumed — there is nothing to retry (P7 deleted the 256-round bind loop).
fn bind_server() -> core::result::Result<KernelServer, nexus_ipc::IpcError> {
    KernelServer::new_with_slots(topo::SERVER.recv, topo::SERVER.send)
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
    /// Bounded S1 rate diagnostics: wheel deltas shipped to windowd.
    wheel_ship_count: u32,
    /// Wheel notches ACCUMULATED until a push actually LANDS: the visible-
    /// state push runs a 2ms-timeout send, and windowd is routinely busy
    /// compositing the very scroll this wheel drives — a failed push must
    /// never drop the notch (input outlives render backpressure, the
    /// Android rule). Cleared only on a successful send.
    pending_wheel_delta: i32,
    /// Bounded diagnostics: visible-state pushes that hit backpressure.
    push_fail_count: u32,
    /// Delivered visible-state pushes in the current ~1s telemetry window.
    push_rate_count: u32,
    hid_rx_rate_window_ns: u64,
    hid_rx_rate_count: u32,
    /// Chord chain hops K2/K3 said once (TASK-0348 P7): a chord recognized, a chord pushed.
    chord_seen_said: bool,
    chord_pushed_said: bool,
    /// Start of the current push-rate telemetry window (0 = not started).
    push_rate_window_ns: u64,
    /// Recycled HID-event decode buffer (capacity reused across batches —
    /// a fresh per-batch `Vec` leaked ~256B/batch on the non-freeing bump
    /// heap and killed input after ~1 min of mousing: `alloc-fail svc=inputd`).
    hid_events_scratch: Vec<HidEvent>,
    /// Recycled WIRE-event decode buffer (same recycling contract one hop
    /// earlier): `decode_push_hid_batch` allocated a fresh `Vec<WireHidEvent>`
    /// per received push frame (~54 B per mouse batch at up to 250 push/s) —
    /// the leak that exhausted inputd's heap in ~30 s of active input and
    /// killed the whole chain ("Absturz").
    wire_events_scratch: Vec<input_live_protocol::WireHidEvent>,
    windowd_push_ok_emitted: bool,
    windowd_push_fail_emitted: bool,
    /// The current compositor outage was named (RFC-0112).
    no_compositor_said: bool,
    wheel_indicator_direction: WheelIndicatorDirection,
    wheel_indicator_deadline_ns: u64,
    windowd_client: Option<KernelClient>,
    /// The key channel (RFC-0113): translation, stream checks, the frame being filled.
    keys: keys::KeyChannel,
    /// RFC-0078 settings-watch: OP_WATCH subscription sent (retried from the
    /// idle arm until the fire-and-forget send succeeds).
    settings_watch_subscribed: bool,
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
        let mut runtime = Self {
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
            wheel_ship_count: 0,
            pending_wheel_delta: 0,
            push_fail_count: 0,
            push_rate_count: 0,
            hid_rx_rate_window_ns: 0,
            hid_rx_rate_count: 0,
            chord_seen_said: false,
            chord_pushed_said: false,
            push_rate_window_ns: 0,
            hid_events_scratch: Vec::new(),
            wire_events_scratch: Vec::new(),
            windowd_push_ok_emitted: false,
            windowd_push_fail_emitted: false,
            no_compositor_said: false,
            wheel_indicator_direction: WheelIndicatorDirection::None,
            wheel_indicator_deadline_ns: 0,
            windowd_client: KernelClient::new_with_slots(topo::WINDOWD.send, topo::WINDOWD.recv)
                .ok(),
            keys: keys::KeyChannel::new(),
            settings_watch_subscribed: false,
            last_windowd_push_state: None,
            last_windowd_push_ns: 0,
            chain: InputdChainTelemetry::new(),
        };
        // RFC-0098 C7: the pointer space is windowd's hit-test space (gpud's mode), asked once.
        let (w, h) = display_space::from_windowd();
        if runtime.input.set_display_space(w, h).is_err() {
            let _ = nexus_abi::debug_write(b"inputd: FAIL display space rejected\n");
        }
        Ok(runtime)
    }

    fn visible_state_snapshot(&mut self) -> VisibleState {
        self.sync_wheel_indicator(nsec().unwrap_or(0));
        self.visible_state
    }

    fn handle_frame(&mut self, frame: &[u8]) -> [u8; 8] {
        if frame_has_op(frame, OP_PUSH_HID_BATCH) {
            if !self.hid_batch_recv_debug_emitted {
                let _ = debug_trace("dbg: inputd hid batch recv");
                // Input-chain hop I3: a wire batch arrived from hidrawd.
                let _ = nexus_abi::trace_line("inputd: chain I3 wire recv from hidrawd");
                self.hid_batch_recv_debug_emitted = true;
            }
            let batch = match decode_push_hid_batch_reusing(
                frame,
                core::mem::take(&mut self.wire_events_scratch),
            ) {
                Ok(batch) => batch,
                Err(scratch) => {
                    // Malformed: recover the scratch capacity, then reject.
                    self.wire_events_scratch = scratch;
                    self.chain.frame_decode_malformed =
                        self.chain.frame_decode_malformed.saturating_add(1);
                    self.chain.hid_malformed = self.chain.hid_malformed.saturating_add(1);
                    // Input-chain hop I4 fail: the wire batch didn't decode.
                    if !self.chain_normalize_fail_emitted {
                        let _ =
                            debug_println("inputd: chain I4 normalize FAIL (malformed wire batch)");
                        self.chain_normalize_fail_emitted = true;
                    }
                    return encode_status(OP_PUSH_HID_BATCH, STATUS_MALFORMED);
                }
            };
            // Input-chain hop I4: the wire batch decoded into normalized events.
            if !self.chain_normalize_ok_emitted {
                let _ = nexus_abi::trace_line("inputd: chain I4 normalized");
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
        self.check_stream(batch.device_id, batch.seq_first, batch.events.len());
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
        let decoded = decode_wire_batch_reusing(
            &batch,
            self.input.pointer_transform(),
            &mut self.hid_events_scratch,
        );
        // The wire events are consumed by the decode above — recycle their
        // buffer NOW (single recovery point, covers the reject early-return
        // too). This closes the LAST per-batch allocation in the chain.
        let mut batch = batch;
        batch.events.clear();
        self.wire_events_scratch = core::mem::take(&mut batch.events);
        let hid_batch = match decoded {
            Ok(batch) => batch,
            Err(reject) => {
                self.chain.record_wire_reject(reject);
                // Alloc-free: a `format!` here would build a String on every
                // reject, at whatever rate an input flood sends them. `label()`
                // is a `&'static str`; print it directly.
                let _ = debug_println(reject.label());
                return reject.status();
            }
        };
        let previous_source = self.input.active_pointer_source();
        let apply_overflowed = self.input.apply_hid_batch_in_place(&hid_batch).is_err();
        // CLOSE THE RECYCLING LOOP: `decode_wire_batch_reusing` took the
        // scratch Vec by `mem::take` — hand its capacity back for the next
        // batch. Dropping the batch instead leaked one events-Vec per batch
        // on the non-freeing bump heap (the SECOND "input dies after minutes
        // of mousing" crash; the first was the per-batch alloc this scratch
        // replaced). EVERY path past decode must run this line.
        self.hid_events_scratch = hid_batch.into_events();
        if apply_overflowed {
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
        // RFC-0113 §3: the keys ride the key channel to windowd (the visible state carries no
        // text) — sent before this batch's state push, on the same channel, in order.
        self.send_keys();
        if !self.absolute_source_debug_emitted
            && matches!(
                active_source,
                Some(PointerSource::TabletAbsolute | PointerSource::TouchAbsolute)
            )
            && active_source != previous_source
        {
            let _ = debug_trace("dbg: inputd active source absolute");
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
            let _ = debug_trace("dbg: inputd relative blocked by absolute source");
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
        // TASK-0346 P2: the modifiers ride every push (a click's Shift extends a selection).
        self.visible_state.modifiers = self.input.modifier_bits();
        let keyboard_held = self.input.held_non_modifier_key_count() > 0;
        let previous_launcher_click = self.visible_state.launcher_click_visible;
        let previous_focus_visible = self.visible_state.focus_visible;
        self.visible_state.cursor_x = display_pointer.x;
        self.visible_state.cursor_y = display_pointer.y;
        if pointer_down_dispatched && !self.pointer_down_dispatch_debug_emitted {
            let _ = debug_trace("dbg: inputd pointer down dispatched");
            self.pointer_down_dispatch_debug_emitted = true;
        }
        if pointer_down_dispatched && !self.pointer_down_delivery_debug_emitted {
            let _ = debug_trace("dbg: inputd pointer down delivered");
            self.pointer_down_delivery_debug_emitted = true;
        }
        if keyboard_dispatched && !self.keyboard_dispatch_debug_emitted {
            let _ = debug_trace("dbg: inputd keyboard dispatched");
            self.keyboard_dispatch_debug_emitted = true;
        }
        if keyboard_dispatched && !self.keyboard_delivery_debug_emitted {
            let _ = debug_trace("dbg: inputd keyboard delivered");
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
                let _ = debug_trace("dbg: inputd focus on target");
                self.focus_debug_emitted = true;
            }
        }
        // Carry the real signed wheel magnitude (0 when no wheel motion this
        // update) so windowd scrolls by the actual notch count, not one quantized
        // step. The pulse booleans below remain a latched direction indicator.
        self.pending_wheel_delta = self.pending_wheel_delta.saturating_add(pointer_wheel_delta);
        self.visible_state.wheel_delta_y = self.pending_wheel_delta;
        if pointer_wheel_delta != 0 && self.wheel_ship_count < 40 {
            self.wheel_ship_count += 1;
            let _ = debug_println(&format!(
                "inputd: wheel ship n={} d={}",
                self.wheel_ship_count, pointer_wheel_delta
            ));
        }
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
            let _ = nexus_abi::trace_line("inputd: live pointer route on");
            self.pointer_marker_emitted = true;
        }
        if self.visible_state.keyboard_route_live && !self.keyboard_marker_emitted {
            let _ = nexus_abi::trace_line("inputd: live keyboard route on");
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
        // (A tiling chord or a capture key went out on the key channel above, TASK-0348 P17 —
        // an event, not a fact of this sample.)
        let immediate_push = pointer_down_dispatched
            || pointer_wheel_delta != 0
            || keyboard_dispatched
            || button_changed
            || focus_changed;
        self.push_visible_state_to_windowd(now_ns, immediate_push);
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
}

fn fail(label: &'static str) -> &'static str {
    let _ = debug_println(label);
    label
}
