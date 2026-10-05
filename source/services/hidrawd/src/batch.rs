// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: hidrawd's one batch path (OS only; TASK-0253B): every source's normalized events
//! of one device become `WireHidBatch` frames to inputd — chunked at `MAX_HID_BATCH_EVENTS`
//! (a burst drain must never be dropped whole), sent without blocking, inputd's acks drained
//! into a stack buffer, a broken route re-made on the next batch. The input chain's first two
//! hops (I1 raw, I2 sent) are said here, for every transport. Reused buffers: nothing is
//! allocated in steady state.
//! OWNERS: @runtime @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: the visible QEMU lanes (input chain I1..I6, the input-flood lane)
//! ADR: docs/adr/0029-input-v1-host-core-architecture.md

extern crate alloc;

use alloc::{format, vec::Vec};

use hid::HidEvent;
use input_live_protocol::{
    encode_push_hid_batch_into, WireHidBatch, WireHidEvent, HID_KIND_KEYBOARD, HID_KIND_MOUSE,
    MAX_HID_BATCH_EVENTS, MAX_HID_BATCH_FRAME_LEN, POINTER_SOURCE_NONE,
};
use nexus_abi::{debug_println, debug_trace};
use nexus_ipc::{Client as _, KernelClient, Wait};

use crate::adapter::wire_event_from_hid;
use crate::source::{DeviceFrame, Emit};
use crate::telemetry::HidrawChainTelemetry;
use crate::{
    classify_live_route_send_error, LiveRouteSendAction, LiveRouteSendErrorClass, PointerSource,
};

/// The batch path to inputd and what it has said so far.
pub(crate) struct Batch {
    client: Option<KernelClient>,
    wire: Vec<WireHidEvent>,
    /// One frame's slice of `wire` (moved into the encoder and back: no allocation).
    chunk: Vec<WireHidEvent>,
    pub(crate) chain: HidrawChainTelemetry,
    raw_said: bool,
    normalized_said: bool,
    sent_said: bool,
    failed_said: bool,
}

impl Batch {
    pub(crate) fn new() -> Self {
        Self {
            client: route_inputd(),
            wire: Vec::with_capacity(64),
            chunk: Vec::with_capacity(MAX_HID_BATCH_EVENTS),
            chain: HidrawChainTelemetry::new(),
            raw_said: false,
            normalized_said: false,
            sent_said: false,
            failed_said: false,
        }
    }

    /// One wire frame to inputd; `false` when the route broke (re-made on the next batch).
    fn send(&mut self, frame: &[u8]) -> bool {
        let Some(client) = self.client.as_ref() else { return false };
        // inputd's acks drained into a stack buffer (an allocating `recv` would keep a `Vec`
        // per ack).
        let mut acks = [0u8; 64];
        while client.recv_into(Wait::NonBlocking, &mut acks).is_ok() {}
        match client.send(frame, Wait::NonBlocking) {
            Ok(()) => {
                if !self.sent_said {
                    let _ = debug_trace("dbg: hidrawd inputd send ok");
                    // Input-chain hop I2: normalized wire batch sent to inputd.
                    let _ = debug_println("hidrawd: chain I2 wire sent to inputd");
                    self.sent_said = true;
                }
                self.chain.sent_batches = self.chain.sent_batches.saturating_add(1);
                self.chain.note_tx_for_rate_line();
                true
            }
            Err(err) => {
                self.chain.send_failures = self.chain.send_failures.saturating_add(1);
                let class = send_error_class(err);
                if !self.failed_said {
                    let _ = debug_println(send_fail_label(class));
                    // Input-chain hop I2 fail: inputd unreachable (reason above).
                    let _ = debug_println("hidrawd: chain I2 wire send FAIL (inputd route)");
                    self.failed_said = true;
                }
                if classify_live_route_send_error(class) == LiveRouteSendAction::ResetRoute {
                    self.client = None;
                    return false;
                }
                true
            }
        }
    }
}

impl Emit for Batch {
    fn emit(&mut self, frame: &DeviceFrame, raw: u16, events: &[HidEvent]) {
        let chain = &mut self.chain;
        chain.raw_batches = chain.raw_batches.saturating_add(1);
        chain.raw_events = chain.raw_events.saturating_add(u64::from(raw));
        chain.note_rx_for_rate_line(u32::from(raw));
        chain.normalized_events = chain.normalized_events.saturating_add(events.len() as u64);
        let counter = match frame.pointer {
            None => &mut chain.keyboard_batches,
            Some(PointerSource::MouseRelative) => &mut chain.mouse_relative_batches,
            Some(PointerSource::TabletAbsolute) => &mut chain.tablet_absolute_batches,
            Some(PointerSource::TouchAbsolute) => &mut chain.touch_absolute_batches,
        };
        *counter = counter.saturating_add(1);
        if !self.raw_said && raw > 0 {
            // Input-chain hop I1: a raw HID event reached us from a device.
            let _ = debug_println("hidrawd: chain I1 device event (raw HID polled)");
            self.raw_said = true;
        }
        if !self.normalized_said && !events.is_empty() {
            let _ = debug_println("hidrawd: ingress adapter ready");
            self.normalized_said = true;
        }
        if events.is_empty() {
            chain.wire_batches_skipped = chain.wire_batches_skipped.saturating_add(1);
            // Bounded triage: WHY did a drain produce no wire events? (raw>0 with norm=0 is the
            // normalize filter.)
            if chain.wire_batches_skipped <= 3 {
                let _ = debug_println(&format!(
                    "hidrawd: wire skip raw={raw} norm=0 role={:?}",
                    frame.pointer
                ));
            }
            return;
        }
        chain.wire_batches = chain.wire_batches.saturating_add(1);
        if self.client.is_none() {
            self.client = route_inputd();
            self.chain.route_rebinds = self.chain.route_rebinds.saturating_add(1);
        }
        self.wire.clear();
        self.wire.extend(events.iter().map(wire_event_from_hid));
        let (device_kind, pointer_source) = match frame.pointer {
            None => (HID_KIND_KEYBOARD, POINTER_SOURCE_NONE),
            Some(source) => (HID_KIND_MOUSE, source.wire_value()),
        };
        // CHUNKED: a burst drain can exceed MAX_HID_BATCH_EVENTS per frame — the encoder refuses
        // an oversize batch, and dropping the whole burst silently was the input-storm collapse
        // (tx=0 while events kept arriving).
        let mut start = 0usize;
        while start < self.wire.len() {
            let end = (start + MAX_HID_BATCH_EVENTS).min(self.wire.len());
            self.chunk.clear();
            self.chunk.extend(self.wire[start..end].iter().copied());
            start = end;
            let len = self.chunk.len() as u16;
            let mut batch = WireHidBatch {
                device_kind,
                device_id: frame.device.raw(),
                pointer_source,
                abs_max_x: frame.abs_max_x,
                abs_max_y: frame.abs_max_y,
                raw_event_count: len,
                normalized_event_count: len,
                events: core::mem::take(&mut self.chunk),
            };
            let mut buf = [0u8; MAX_HID_BATCH_FRAME_LEN];
            let encoded = encode_push_hid_batch_into(&batch, &mut buf);
            self.chunk = core::mem::take(&mut batch.events);
            let Some(n) = encoded else { continue };
            if !self.send(&buf[..n]) {
                break;
            }
        }
    }
}

/// The declared inputd leg (TASK-0324 P4d) — pinned before hidrawd runs; no route ask.
fn route_inputd() -> Option<KernelClient> {
    let leg = nexus_service_topology::slots::hidrawd::INPUTD;
    KernelClient::new_with_slots(leg.send, leg.recv).ok()
}

fn send_error_class(err: nexus_ipc::IpcError) -> LiveRouteSendErrorClass {
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

const fn send_fail_label(class: LiveRouteSendErrorClass) -> &'static str {
    match class {
        LiveRouteSendErrorClass::Backpressure => "dbg: hidrawd inputd send fail backpressure",
        LiveRouteSendErrorClass::Disconnected => "dbg: hidrawd inputd send fail disconnected",
        LiveRouteSendErrorClass::Fatal => "dbg: hidrawd inputd send fail fatal",
    }
}
