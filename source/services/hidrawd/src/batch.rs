// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: hidrawd's one batch path (OS only; TASK-0253B): every source's normalized events
//! of one device become `WireHidBatch` frames to inputd — chunked at `MAX_HID_BATCH_EVENTS`
//! (a burst drain must never be dropped whole), each chunk numbered in its device's stream
//! (`seq_first`, RFC-0113 §1: every hop after this one counts a gap), sent BLOCKING (RFC-0113
//! §2: a full inputd queue holds hidrawd — it used to drop the batch, and the keys in it, for
//! good; inputd never waits on hidrawd, so the wait cannot close a cycle), inputd's acks drained
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
    encode_push_hid_batch_into, SeqStreams, WireHidBatch, WireHidEvent, HID_KIND_KEYBOARD,
    HID_KIND_MOUSE, MAX_HID_BATCH_EVENTS, MAX_HID_BATCH_FRAME_LEN, POINTER_SOURCE_NONE,
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
    /// Every device's stream sequence: the next number of each (RFC-0113 §1).
    streams: SeqStreams,
    pub(crate) chain: HidrawChainTelemetry,
    raw_said: bool,
    normalized_said: bool,
    /// Chord chain hop K1 said once (TASK-0348 P7): a keyboard's events were normalized.
    keyboard_said: bool,
    sent_said: bool,
    failed_said: bool,
    /// Triage lines said for drains that normalized to nothing (bounded per boot).
    skip_lines: u8,
}

impl Batch {
    pub(crate) fn new() -> Self {
        Self {
            client: route_inputd(),
            wire: Vec::with_capacity(64),
            chunk: Vec::with_capacity(MAX_HID_BATCH_EVENTS),
            streams: SeqStreams::new(),
            chain: HidrawChainTelemetry::new(),
            raw_said: false,
            normalized_said: false,
            keyboard_said: false,
            sent_said: false,
            failed_said: false,
            skip_lines: 0,
        }
    }

    /// One wire frame to inputd; `false` when the route broke (re-made on the next batch). A
    /// pointer frame counts in the rate line; a keyboard frame in nothing per window.
    fn send(&mut self, frame: &[u8], pointer: bool) -> bool {
        let Some(client) = self.client.as_ref() else { return false };
        // inputd's acks drained into a stack buffer (an allocating `recv` would keep a `Vec`
        // per ack).
        let mut acks = [0u8; 64];
        while client.recv_into(Wait::NonBlocking, &mut acks).is_ok() {}
        match client.send(frame, Wait::Blocking) {
            Ok(()) => {
                if !self.sent_said {
                    let _ = debug_trace("dbg: hidrawd inputd send ok");
                    // Input-chain hop I2: normalized wire batch sent to inputd.
                    let _ = debug_println("hidrawd: chain I2 wire sent to inputd");
                    self.sent_said = true;
                }
                if pointer {
                    self.chain.sent_batches = self.chain.sent_batches.saturating_add(1);
                    self.chain.note_tx_for_rate_line();
                }
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
        // The per-window counters see POINTER traffic only: keyboard batches per second are the
        // rhythm and the length of what was typed — a password's too (RFC-0113 §6, the
        // keystroke rule of SECURITY_STANDARDS §1).
        let pointer = frame.pointer.is_some();
        let chain = &mut self.chain;
        if let Some(source) = frame.pointer {
            chain.raw_batches = chain.raw_batches.saturating_add(1);
            chain.raw_events = chain.raw_events.saturating_add(u64::from(raw));
            chain.note_rx_for_rate_line(u32::from(raw));
            chain.normalized_events = chain.normalized_events.saturating_add(events.len() as u64);
            let counter = match source {
                PointerSource::MouseRelative => &mut chain.mouse_relative_batches,
                PointerSource::TabletAbsolute => &mut chain.tablet_absolute_batches,
                PointerSource::TouchAbsolute => &mut chain.touch_absolute_batches,
            };
            *counter = counter.saturating_add(1);
        }
        if !self.raw_said && raw > 0 {
            // Input-chain hop I1: a raw HID event reached us from a device.
            let _ = debug_println("hidrawd: chain I1 device event (raw HID polled)");
            self.raw_said = true;
        }
        if !self.normalized_said && !events.is_empty() {
            let _ = debug_println("hidrawd: ingress adapter ready");
            self.normalized_said = true;
        }
        if !self.keyboard_said && !pointer && !events.is_empty() {
            // Chord chain hop K1 (TASK-0348 P7): a keyboard's events are normalized and go out
            // on the batch path below (what they were is not said — RFC-0113 §6).
            let _ = debug_println("hidrawd: chain K1 keyboard events normalized");
            self.keyboard_said = true;
        }
        if events.is_empty() {
            if pointer {
                chain.wire_batches_skipped = chain.wire_batches_skipped.saturating_add(1);
            }
            // Bounded triage (three lines a boot): WHY did a drain produce no wire events?
            // (raw>0 with norm=0 is the normalize filter.)
            if self.skip_lines < 3 {
                self.skip_lines += 1;
                let _ = debug_println(&format!(
                    "hidrawd: wire skip raw={raw} norm=0 role={:?}",
                    frame.pointer
                ));
            }
            return;
        }
        if pointer {
            chain.wire_batches = chain.wire_batches.saturating_add(1);
        }
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
            // Numbered before the send: a chunk that never arrives is a gap inputd names.
            let seq_first = self.streams.stamp(frame.device.raw(), u32::from(len));
            let mut batch = WireHidBatch {
                device_kind,
                device_id: frame.device.raw(),
                pointer_source,
                abs_max_x: frame.abs_max_x,
                abs_max_y: frame.abs_max_y,
                raw_event_count: len,
                normalized_event_count: len,
                seq_first,
                events: core::mem::take(&mut self.chunk),
            };
            let mut buf = [0u8; MAX_HID_BATCH_FRAME_LEN];
            let encoded = encode_push_hid_batch_into(&batch, &mut buf);
            self.chunk = core::mem::take(&mut batch.events);
            let Some(n) = encoded else { continue };
            if !self.send(&buf[..n], pointer) {
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
