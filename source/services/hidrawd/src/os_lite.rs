// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: OS-lite `hidrawd` live backend owning virtio-input MMIO windows.
//! OWNERS: @runtime @ui
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `cargo test -p hidrawd -- --nocapture`
//! ADR: docs/adr/0029-input-v1-host-core-architecture.md

extern crate alloc;

use alloc::{format, vec::Vec};
use core::time::Duration;

use hid::{HidEvent, TimestampNs};
use input_live_protocol::{
    encode_push_hid_batch_into, WireHidBatch, WireHidEvent, HID_KIND_KEYBOARD, HID_KIND_MOUSE,
    MAX_HID_BATCH_EVENTS, MAX_HID_BATCH_FRAME_LEN, POINTER_SOURCE_NONE,
};
use nexus_abi::{
    cap_clone, cap_close, debug_println, debug_trace, ipc_recv_v1, irq_bind, irq_complete, nsec,
    yield_, Cap, MsgHeader, IPC_SYS_TRUNCATE,
};
use nexus_ipc::budget::{route_with_nonce_budgeted, NonceMismatchBudget, RouteRetryOutcome};
use nexus_ipc::{Client as _, KernelClient, Wait};
use virtio_input::{
    DeviceRole, DeviceSlot, InputEventKind, MappedVirtioInputDevice, RawInputEvent,
};

use crate::{
    classify_live_route_send_error, normalize_ingress_into, resolve_absolute_axis_max, DeviceId,
    HidrawdService, IngressGateEvidence, IngressRole, LiveRouteSendAction, LiveRouteSendErrorClass,
    PointerSource, RawIngressEvent, RawIngressEventKind,
};

const INPUT_CAP_SLOTS: [u32; 3] = [50, 51, 52];
const INPUT_MMIO_VAS: [usize; 3] = [0x2003_0000, 0x2003_1000, 0x2003_2000];
const INPUT_QUEUE_VAS: [usize; 3] = [0x2004_0000, 0x2005_0000, 0x2006_0000];
const INPUT_BUFFER_VAS: [usize; 3] = [0x2007_0000, 0x2008_0000, 0x2009_0000];

use crate::telemetry::HidrawChainTelemetry;

pub fn service_main_loop() -> Result<(), nexus_abi::AbiError> {
    // NOTE: hidrawd is currently resumed by init BEFORE its input MMIO is granted + the inputd
    // route is wired, so the loop below busy-yields until both land (measured by `load_span`).
    let load_span = nexus_abi::Span::begin();
    let mut service = HidrawdService::new();
    // The live loop reads input via `normalize_ingress_into` and never inspects
    // `recent_batches`; recording there only burns the non-freeing bump heap.
    service.disable_recent_recording();
    // Reusable per-poll buffers — cleared + refilled each iteration so the hot path
    // allocates nothing in steady state (the hidrawd OOM fix, "maus kaum benutzbar").
    let mut scratch = IngressScratch::new();
    let mut missing_slots_logged = [false; INPUT_CAP_SLOTS.len()];
    let mut live_devices = open_live_devices(&mut missing_slots_logged);
    let mut client = route_inputd_blocking();
    let mut ready_emitted = false;
    let mut payload_ready_emitted = false;
    let mut raw_gate_emitted = false;
    let mut normalized_gate_emitted = false;
    let mut send_ok_emitted = false;
    let mut send_fail_emitted = false;
    let mut chain = HidrawChainTelemetry::new();
    // Reactive input: endpoint the kernel routes device IRQs to (via irq_bind), so
    // we block on it instead of busy-polling the virtio-input queues. Bound lazily
    // once devices are open.
    let mut irq_endpoint: Option<Cap> = None;

    loop {
        if !payload_ready_emitted {
            debug_println("hidrawd: os service payload ready")?;
            payload_ready_emitted = true;
        }
        // Caps are guaranteed after initial yield — reprobe is only needed
        // if a device was hot-unplugged (rare). Simple bounded retry.
        if live_devices.is_empty() {
            live_devices = open_live_devices(&mut missing_slots_logged);
        }
        if client.is_none() {
            client = route_inputd_blocking();
            if client.is_none() {
                chain.idle_yields = chain.idle_yields.saturating_add(1);
                chain.report_if_due();
                let _ = yield_();
                continue;
            }
            chain.route_rebinds = chain.route_rebinds.saturating_add(1);
        }
        if !ready_emitted && !live_devices.is_empty() {
            debug_println("hidrawd: ready")?;
            let _ = debug_println(&format!(
                "hidrawd: timing entry_to_ready_ms={}",
                load_span.elapsed_ms()
            ));
            // RFC-0068: ready reached — emit the folded `hidrawd N/N` verdict (interactive only).
            nexus_abi::service_verdict_flush("hidrawd");
            ready_emitted = true;
        }
        if live_devices.is_empty() {
            chain.idle_yields = chain.idle_yields.saturating_add(1);
            chain.report_if_due();
            // No input devices to service (e.g. a headless lane with no
            // virtio-input): PARK off the run queue on a bounded deadline instead
            // of yield-spinning at Normal QoS. A busy-yield here kept the Normal
            // queue perpetually non-empty on the strict-priority scheduler and
            // starved Idle background work (netstackd bootstrap, selftest OTA) —
            // the ~12k idle_yields/window class. We park on our control endpoint
            // (slot 2, owned + otherwise idle); a control message OR the deadline
            // wakes us to re-probe for hot-plugged devices.
            idle_park(HIDRAWD_IDLE_PARK_NS);
            continue;
        }

        // Bind each device's PLIC IRQ to our control-reply endpoint (slot 2),
        // which we already own + recv on and which is idle after routing. The
        // kernel then wakes us reactively on input (irq_bind) instead of polling.
        // (A dedicated endpoint via init's EndpointFactory is a later refinement;
        // plain ipc_endpoint_create is a deprecated, permission-denied ABI.)
        if irq_endpoint.is_none() {
            const IRQ_NOTIFY_SLOT: Cap = 2;
            let mut bound_any = false;
            for device in &live_devices {
                if irq_bind(device.irq, IRQ_NOTIFY_SLOT).is_ok() {
                    bound_any = true;
                }
            }
            if bound_any {
                irq_endpoint = Some(IRQ_NOTIFY_SLOT);
                debug_println("hidrawd: irq endpoint bound (reactive input)")?;
            }
        }

        chain.note_wake_for_rate_line();
        let mut sent_any = false;
        // Level-triggered drain protocol (the input-rate ceiling fix): ACK the
        // ISR latch FIRST, then drain — an event landing during the drain sets
        // the latch FRESH, so it is either caught by the next pass of this loop
        // or stays pending for immediate redelivery after `irq_complete` below.
        // The old order (drain, then ack) wiped exactly those late events'
        // latch: with QEMU refilling the ring the moment we requeue, the chain
        // collapsed to backstop-paced ~30 wakes/s under a move storm.
        'drain: loop {
            for device in &live_devices {
                device.driver.ack_interrupt();
            }
            let mut polled_any = false;
            for device in &mut live_devices {
                let Some(polled) = device.poll_batch(&mut service, &mut scratch) else {
                    continue;
                };
                polled_any = true;
                chain.raw_batches = chain.raw_batches.saturating_add(1);
                chain.raw_events =
                    chain.raw_events.saturating_add(u64::from(polled.evidence.raw_event_count()));
                chain.note_rx_for_rate_line(u32::from(polled.evidence.raw_event_count()));
                chain.normalized_events = chain
                    .normalized_events
                    .saturating_add(u64::from(polled.evidence.normalized_event_count()));
                match polled.pointer_source {
                    None => chain.keyboard_batches = chain.keyboard_batches.saturating_add(1),
                    Some(PointerSource::MouseRelative) => {
                        chain.mouse_relative_batches =
                            chain.mouse_relative_batches.saturating_add(1)
                    }
                    Some(PointerSource::TabletAbsolute) => {
                        chain.tablet_absolute_batches =
                            chain.tablet_absolute_batches.saturating_add(1)
                    }
                    Some(PointerSource::TouchAbsolute) => {
                        chain.touch_absolute_batches =
                            chain.touch_absolute_batches.saturating_add(1)
                    }
                }
                if !raw_gate_emitted && polled.evidence.raw_event_count() > 0 {
                    debug_println("hidrawd: virtio-input raw event seen")?;
                    // Input-chain hop I1: a raw HID event reached us from the device.
                    debug_println("hidrawd: chain I1 device event (raw HID polled)")?;
                    raw_gate_emitted = true;
                }
                if !normalized_gate_emitted && polled.evidence.normalized_event_count() > 0 {
                    debug_println("hidrawd: ingress adapter ready")?;
                    normalized_gate_emitted = true;
                }
                let Some(meta) = polled.wire_meta else {
                    chain.wire_batches_skipped = chain.wire_batches_skipped.saturating_add(1);
                    // Bounded triage: WHY does a polled batch produce no wire
                    // events? (raw>0 with norm=0 = the normalize filter; the
                    // storm-collapse signature tx=0.)
                    if chain.wire_batches_skipped <= 3 {
                        let _ = debug_println(&format!(
                            "hidrawd: wire skip raw={} norm={} role={:?}",
                            polled.evidence.raw_event_count(),
                            polled.evidence.normalized_event_count(),
                            polled.pointer_source,
                        ));
                    }
                    continue;
                };
                chain.wire_batches = chain.wire_batches.saturating_add(1);
                // CHUNKED send: a burst drain (device-ring backlog) can exceed
                // MAX_HID_BATCH_EVENTS per frame — the encoder returns None for
                // oversize batches, and dropping the whole burst silently was
                // the input-storm collapse (tx=0 while events kept arriving).
                // The reusable chunk Vec keeps the hot path alloc-free.
                let mut chunk_start = 0usize;
                while chunk_start < scratch.wire.len() {
                    let chunk_end = (chunk_start + MAX_HID_BATCH_EVENTS).min(scratch.wire.len());
                    scratch.wire_chunk.clear();
                    scratch.wire_chunk.extend(scratch.wire[chunk_start..chunk_end].iter().copied());
                    let chunk_len = (chunk_end - chunk_start) as u16;
                    let mut frame_buf = [0u8; MAX_HID_BATCH_FRAME_LEN];
                    // Move the reusable chunk buffer into a transient batch for
                    // encoding, then straight back (capacity survives, no alloc).
                    let mut batch = WireHidBatch {
                        device_kind: meta.device_kind,
                        device_id: meta.device_id,
                        pointer_source: meta.pointer_source,
                        abs_max_x: meta.abs_max_x,
                        abs_max_y: meta.abs_max_y,
                        raw_event_count: chunk_len,
                        normalized_event_count: chunk_len,
                        events: core::mem::take(&mut scratch.wire_chunk),
                    };
                    let encoded = encode_push_hid_batch_into(&batch, &mut frame_buf);
                    scratch.wire_chunk = core::mem::take(&mut batch.events);
                    chunk_start = chunk_end;
                    let Some(frame_len) = encoded else {
                        continue;
                    };
                    let frame = &frame_buf[..frame_len];
                    let Some(current_client) = client.as_ref() else {
                        break 'drain;
                    };
                    // Drain inputd's acks into a stack buffer (the allocating `recv`
                    // would leak a `Vec` per ack on the non-freeing bump heap).
                    let mut drain = [0u8; 64];
                    while current_client.recv_into(Wait::NonBlocking, &mut drain).is_ok() {}
                    match current_client.send(&frame, Wait::NonBlocking) {
                        Ok(()) => {
                            if !send_ok_emitted {
                                debug_trace("dbg: hidrawd inputd send ok")?;
                                // Input-chain hop I2: normalized wire batch sent to inputd.
                                debug_println("hidrawd: chain I2 wire sent to inputd")?;
                                send_ok_emitted = true;
                            }
                        }
                        Err(err) => {
                            chain.send_failures = chain.send_failures.saturating_add(1);
                            if !send_fail_emitted {
                                debug_println(live_route_send_fail_label(err))?;
                                // Input-chain hop I2 fail: inputd unreachable (reason above).
                                debug_println("hidrawd: chain I2 wire send FAIL (inputd route)")?;
                                send_fail_emitted = true;
                            }
                            if classify_live_route_send_error(map_live_route_send_error(err))
                                == LiveRouteSendAction::ResetRoute
                            {
                                client = None;
                                break 'drain;
                            }
                            continue;
                        }
                    }
                    chain.sent_batches = chain.sent_batches.saturating_add(1);
                    chain.note_tx_for_rate_line();
                    sent_any = true;
                }
            }
            // A pass that drained nothing after an ack ⇒ ring empty AND latch
            // clear — safe to unmask + park. (Bounded: each pass consumes real
            // ring entries; an idle chain exits on its first pass.)
            if !polled_any {
                break 'drain;
            }
        }

        // Reactive idle: the ISR latch was acked BEFORE the final (empty) drain
        // pass above; re-arm the PLIC source and BLOCK until the next device
        // IRQ. The kernel routes the virtio-input IRQ to our endpoint
        // (immediately via S_EXT, or within a tick via the timer backstop) and
        // wakes this recv — no busy-poll.
        chain.report_if_due();
        if let Some(ep) = irq_endpoint {
            for device in &live_devices {
                let _ = irq_complete(device.irq);
            }
            let mut hdr = MsgHeader::new(0, 0, 0, 0, 0);
            let mut buf = [0u8; 32];
            // Blocking recv (no NONBLOCK): parks until a device IRQ notification.
            let _ = ipc_recv_v1(ep, &mut hdr, &mut buf, IPC_SYS_TRUNCATE, 0);
        } else if !sent_any {
            chain.idle_yields = chain.idle_yields.saturating_add(1);
            // Devices present but no IRQ endpoint bound yet + nothing to send:
            // park on a bounded deadline instead of yield-spinning (same Idle-
            // starvation reason as the empty-devices path above).
            idle_park(HIDRAWD_IDLE_PARK_NS);
        }
    }
}

/// Idle back-off for the hidrawd loop when there is nothing to service — 50ms.
/// Bounded so hot-plugged devices are picked up within a re-probe cycle while the
/// service stays OFF the run queue between wakes (no Normal-QoS busy-yield that
/// would starve Idle background work on the strict-priority scheduler).
const HIDRAWD_IDLE_PARK_NS: u64 = 50_000_000;

/// PARK the current task for up to `park_ns` (bounded deadline) instead of
/// busy-yielding: a blocking recv on the owned control endpoint (slot 2) with a
/// deadline. A control message OR the deadline wakes it; either way it takes zero
/// CPU while parked. Replaces `yield_()` in the hidrawd idle paths.
fn idle_park(park_ns: u64) {
    const CONTROL_REPLY_SLOT: Cap = 2;
    let deadline = nsec().unwrap_or(0).saturating_add(park_ns);
    let mut hdr = MsgHeader::new(0, 0, 0, 0, 0);
    let mut buf = [0u8; 32];
    let _ = ipc_recv_v1(CONTROL_REPLY_SLOT, &mut hdr, &mut buf, IPC_SYS_TRUNCATE, deadline);
}

fn map_live_route_send_error(err: nexus_ipc::IpcError) -> LiveRouteSendErrorClass {
    match err {
        nexus_ipc::IpcError::WouldBlock
        | nexus_ipc::IpcError::Timeout
        | nexus_ipc::IpcError::NoSpace => LiveRouteSendErrorClass::Backpressure,
        nexus_ipc::IpcError::Disconnected
        | nexus_ipc::IpcError::Kernel(nexus_abi::IpcError::NoSuchEndpoint) => {
            LiveRouteSendErrorClass::Disconnected
        }
        _ => LiveRouteSendErrorClass::Fatal,
    }
}

fn live_route_send_fail_label(err: nexus_ipc::IpcError) -> &'static str {
    match map_live_route_send_error(err) {
        LiveRouteSendErrorClass::Backpressure => "dbg: hidrawd inputd send fail backpressure",
        LiveRouteSendErrorClass::Disconnected => "dbg: hidrawd inputd send fail disconnected",
        LiveRouteSendErrorClass::Fatal => "dbg: hidrawd inputd send fail fatal",
    }
}

fn open_live_devices(missing_slots_logged: &mut [bool; INPUT_CAP_SLOTS.len()]) -> Vec<LiveDevice> {
    let mut devices = Vec::new();
    let mut emitted_mmio_ready = false;
    for (idx, slot) in INPUT_CAP_SLOTS.into_iter().enumerate() {
        if !slot_present(slot) {
            if !missing_slots_logged[idx] {
                let _ = debug_println(&format!("hidrawd: input slot missing {}", slot));
                missing_slots_logged[idx] = true;
            }
            continue;
        }
        if missing_slots_logged[idx] {
            let _ = debug_println(&format!("hidrawd: input slot ready {}", slot));
            missing_slots_logged[idx] = false;
        }
        let driver = match MappedVirtioInputDevice::open(
            slot,
            INPUT_MMIO_VAS[idx],
            INPUT_QUEUE_VAS[idx],
            INPUT_BUFFER_VAS[idx],
            DeviceSlot::new(idx as u8),
        ) {
            Ok(driver) => driver,
            Err(err) => {
                let _ = debug_println(&format!("hidrawd: input open fail slot={} err={err}", slot));
                continue;
            }
        };
        if !emitted_mmio_ready {
            let _ = debug_println("hidrawd: virtio-input mmio ready");
            emitted_mmio_ready = true;
        }
        let device_id = DeviceId::new((idx + 1) as u16);
        let abs_max_x = driver.absolute_x().map_or(0, |info| info.max());
        let abs_max_y = driver.absolute_y().map_or(0, |info| info.max());
        let provisional_class = match driver.role() {
            DeviceRole::Keyboard => LiveDeviceClass::Keyboard,
            DeviceRole::AbsolutePointer => LiveDeviceClass::Pointer(PointerSource::TabletAbsolute),
            DeviceRole::RelativePointer if abs_max_x > 0 && abs_max_y > 0 => {
                LiveDeviceClass::Pointer(PointerSource::TabletAbsolute)
            }
            DeviceRole::RelativePointer => LiveDeviceClass::Pointer(PointerSource::MouseRelative),
        };
        devices.push(LiveDevice {
            driver,
            device_id,
            provisional_class,
            confirmed_class: None,
            abs_max_x,
            abs_max_y,
            // cap-slot index `idx` => virtio-mmio slot 2+idx => PLIC source 3+idx.
            irq: 3 + idx as u32,
        });
    }
    devices
}

fn slot_present(slot: u32) -> bool {
    match cap_clone(slot) {
        Ok(tmp) => {
            let _ = cap_close(tmp);
            true
        }
        Err(_) => false,
    }
}

fn route_inputd_blocking() -> Option<KernelClient> {
    const CTRL_SEND_SLOT: u32 = 1;
    const CTRL_RECV_SLOT: u32 = 2;
    match route_with_nonce_budgeted(
        b"inputd",
        CTRL_SEND_SLOT,
        CTRL_RECV_SLOT,
        Duration::from_secs(2),
        NonceMismatchBudget::new(64),
    ) {
        RouteRetryOutcome::Success { send_slot, recv_slot } => {
            KernelClient::new_with_slots(send_slot, recv_slot).ok()
        }
        _ => None,
    }
}

struct LiveDevice {
    driver: MappedVirtioInputDevice,
    device_id: DeviceId,
    provisional_class: LiveDeviceClass,
    confirmed_class: Option<LiveDeviceClass>,
    abs_max_x: i32,
    abs_max_y: i32,
    /// PLIC interrupt source for this device. QEMU virt wires virtio-mmio slot N
    /// (0x10001000 + N*0x1000) to source `1 + N`; the input devices are granted at
    /// mmio slots 2/3 (cap-slot index 0/1), i.e. sources 3/4.
    irq: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LiveDeviceClass {
    Keyboard,
    Pointer(PointerSource),
}

impl LiveDevice {
    /// Poll one device into the shared reusable `scratch` buffers (zero steady-state
    /// alloc). On success `scratch.wire` holds the normalized wire events; the returned
    /// frame carries the `WireHidBatch` header fields the caller needs to emit them.
    fn poll_batch(
        &mut self,
        service: &mut HidrawdService,
        scratch: &mut IngressScratch,
    ) -> Option<PolledDeviceFrame> {
        // Drain the device used-ring into the reusable raw buffer; bail if nothing new.
        if !self.driver.poll_batch_into(&mut scratch.raw_input).ok()? {
            return None;
        }
        let timestamp = TimestampNs::new(nsec().unwrap_or(0));
        scratch.raw_ingress.clear();
        scratch.raw_ingress.extend(scratch.raw_input.iter().copied().map(raw_ingress_event));
        let active_class =
            infer_device_class(self.provisional_class, self.confirmed_class, &scratch.raw_ingress);
        let active_pointer_source = pointer_source_for_class(active_class);
        let active_role = ingress_role_for_source(active_pointer_source);
        if self.confirmed_class != Some(active_class) {
            match active_class {
                LiveDeviceClass::Keyboard => {
                    service.register_keyboard(self.device_id);
                    let _ = debug_println("hidrawd: device kbd");
                    let _ = debug_println("hidrawd: virtio-input keyboard ready");
                }
                LiveDeviceClass::Pointer(PointerSource::MouseRelative) => {
                    service.register_mouse(self.device_id);
                    let _ = debug_println("hidrawd: device mouse");
                    let _ = debug_println("hidrawd: source mouse-relative");
                    let _ = debug_println("hidrawd: virtio-input pointer ready");
                }
                LiveDeviceClass::Pointer(PointerSource::TabletAbsolute) => {
                    service.register_mouse(self.device_id);
                    let _ = debug_println("hidrawd: device tablet");
                    let _ = debug_println("hidrawd: source tablet-absolute");
                    let _ = debug_println("hidrawd: virtio-input pointer ready");
                }
                LiveDeviceClass::Pointer(PointerSource::TouchAbsolute) => {
                    service.register_mouse(self.device_id);
                    let _ = debug_println("hidrawd: device touch");
                    let _ = debug_println("hidrawd: source touch-absolute");
                    let _ = debug_println("hidrawd: virtio-input pointer ready");
                }
            }
            self.confirmed_class = Some(active_class);
        }
        self.abs_max_x = resolve_absolute_axis_max(
            active_pointer_source,
            self.abs_max_x,
            &scratch.raw_ingress,
            0,
        );
        self.abs_max_y = resolve_absolute_axis_max(
            active_pointer_source,
            self.abs_max_y,
            &scratch.raw_ingress,
            1,
        );
        // Disjoint field borrows: `raw_ingress` (read) + `hid`/`wire` (written).
        let evidence = normalize_ingress_into(
            active_role,
            &scratch.raw_ingress,
            timestamp,
            &mut scratch.hid,
            &mut scratch.wire,
        );
        let wire_meta = (evidence.normalized_event_count() > 0).then(|| WireMeta {
            device_kind: wire_kind_for(active_role),
            device_id: self.device_id.raw(),
            pointer_source: active_pointer_source
                .map_or(POINTER_SOURCE_NONE, PointerSource::wire_value),
            abs_max_x: self.abs_max_x,
            abs_max_y: self.abs_max_y,
            raw_event_count: evidence.raw_event_count(),
            normalized_event_count: evidence.normalized_event_count(),
        });
        Some(PolledDeviceFrame { evidence, pointer_source: active_pointer_source, wire_meta })
    }
}

/// Reusable per-poll scratch buffers for the live ingest loop. Each is `clear()`ed
/// and refilled per poll, so capacity is retained and the hot path allocates nothing
/// in steady state (the hidrawd OOM fix). Pre-sized to comfortably hold one frame
/// (`MAX_HID_BATCH_FRAME_LEN` bounds a batch to ≤15 events).
struct IngressScratch {
    raw_input: Vec<RawInputEvent>,
    raw_ingress: Vec<RawIngressEvent>,
    hid: Vec<HidEvent>,
    wire: Vec<WireHidEvent>,
    /// Reusable per-frame chunk of `wire` (burst drains split at
    /// `MAX_HID_BATCH_EVENTS` — see the chunked send).
    wire_chunk: Vec<WireHidEvent>,
}

impl IngressScratch {
    fn new() -> Self {
        Self {
            raw_input: Vec::with_capacity(64),
            wire_chunk: Vec::with_capacity(MAX_HID_BATCH_EVENTS),
            raw_ingress: Vec::with_capacity(64),
            hid: Vec::with_capacity(64),
            wire: Vec::with_capacity(64),
        }
    }
}

#[derive(Clone, Copy)]
struct PolledDeviceFrame {
    evidence: IngressGateEvidence,
    pointer_source: Option<PointerSource>,
    /// `Some` when `scratch.wire` holds events to emit; carries the wire header fields.
    wire_meta: Option<WireMeta>,
}

#[derive(Clone, Copy)]
struct WireMeta {
    device_kind: u8,
    device_id: u16,
    pointer_source: u8,
    abs_max_x: i32,
    abs_max_y: i32,
    raw_event_count: u16,
    normalized_event_count: u16,
}

fn infer_device_class(
    provisional_class: LiveDeviceClass,
    confirmed_class: Option<LiveDeviceClass>,
    raw_events: &[RawIngressEvent],
) -> LiveDeviceClass {
    if let Some(class) = confirmed_class {
        return class;
    }
    if raw_events.iter().any(|event| event.kind() == RawIngressEventKind::Absolute) {
        return match provisional_class {
            LiveDeviceClass::Pointer(PointerSource::TouchAbsolute) => {
                LiveDeviceClass::Pointer(PointerSource::TouchAbsolute)
            }
            _ => LiveDeviceClass::Pointer(PointerSource::TabletAbsolute),
        };
    }
    if raw_events.iter().any(|event| event.kind() == RawIngressEventKind::Relative) {
        return LiveDeviceClass::Pointer(PointerSource::MouseRelative);
    }
    if raw_events
        .iter()
        .any(|event| event.kind() == RawIngressEventKind::Key && event.code() >= 0x110)
    {
        return LiveDeviceClass::Pointer(PointerSource::MouseRelative);
    }
    provisional_class
}

const fn pointer_source_for_class(class: LiveDeviceClass) -> Option<PointerSource> {
    match class {
        LiveDeviceClass::Keyboard => None,
        LiveDeviceClass::Pointer(source) => Some(source),
    }
}

const fn ingress_role_for_source(pointer_source: Option<PointerSource>) -> IngressRole {
    match pointer_source {
        None => IngressRole::Keyboard,
        Some(PointerSource::MouseRelative) => IngressRole::RelativePointer,
        Some(PointerSource::TabletAbsolute) => IngressRole::AbsolutePointer,
        Some(PointerSource::TouchAbsolute) => IngressRole::AbsolutePointer,
    }
}

fn wire_kind_for(role: IngressRole) -> u8 {
    match role {
        IngressRole::Keyboard => HID_KIND_KEYBOARD,
        IngressRole::RelativePointer | IngressRole::AbsolutePointer => HID_KIND_MOUSE,
    }
}

fn raw_ingress_event(event: RawInputEvent) -> RawIngressEvent {
    let kind = match event.kind() {
        InputEventKind::Key => RawIngressEventKind::Key,
        InputEventKind::Relative => RawIngressEventKind::Relative,
        InputEventKind::Absolute => RawIngressEventKind::Absolute,
        InputEventKind::Syn | InputEventKind::Unknown(_) => RawIngressEventKind::Key,
    };
    RawIngressEvent::new(kind, event.code(), event.value())
}
