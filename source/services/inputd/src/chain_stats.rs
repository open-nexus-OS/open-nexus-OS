// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: inputd chain telemetry — the folded `fps:` counter dump for the
//! input-normalization chain. Split out of `os_lite.rs` (structure-gate).

extern crate alloc;

use alloc::format;
use input_live_protocol::{
    VisibleState, STATUS_MALFORMED, STATUS_OK, STATUS_OVERFLOW, STATUS_UNSUPPORTED,
};
use nexus_abi::{debug_trace, nsec};

use crate::WireBatchReject;

pub(crate) struct InputdChainTelemetry {
    pub(crate) last_report_ns: u64,
    pub(crate) total_frames: u64,
    pub(crate) hid_push_frames: u64,
    pub(crate) visible_state_polls: u64,
    pub(crate) visible_state_replies: u64,
    pub(crate) unsupported_frames: u64,
    pub(crate) hid_ok: u64,
    pub(crate) hid_malformed: u64,
    pub(crate) hid_unsupported: u64,
    pub(crate) hid_overflow: u64,
    pub(crate) frame_decode_malformed: u64,
    pub(crate) wire_count_rejects: u64,
    pub(crate) wire_device_kind_rejects: u64,
    pub(crate) wire_pointer_source_rejects: u64,
    pub(crate) wire_event_kind_rejects: u64,
    pub(crate) wire_source_mode_rejects: u64,
    pub(crate) wire_abs_calibration_rejects: u64,
    pub(crate) wire_abs_axis_rejects: u64,
    pub(crate) route_overflow_apply: u64,
    pub(crate) route_overflow_delivery: u64,
    pub(crate) raw_events: u64,
    pub(crate) normalized_events: u64,
    pub(crate) dispatch_events: u64,
    pub(crate) delivered_events: u64,
    pub(crate) pointer_dispatch_batches: u64,
    pub(crate) keyboard_dispatch_batches: u64,
    pub(crate) pointer_delivery_batches: u64,
    pub(crate) keyboard_delivery_batches: u64,
    pub(crate) idle_yields: u64,
}

impl InputdChainTelemetry {
    const REPORT_INTERVAL_NS: u64 = 1_000_000_000;

    pub(crate) fn new() -> Self {
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

    pub(crate) fn record_hid_status(&mut self, status: u8) {
        match status {
            STATUS_OK => self.hid_ok = self.hid_ok.saturating_add(1),
            STATUS_MALFORMED => self.hid_malformed = self.hid_malformed.saturating_add(1),
            STATUS_UNSUPPORTED => self.hid_unsupported = self.hid_unsupported.saturating_add(1),
            STATUS_OVERFLOW => self.hid_overflow = self.hid_overflow.saturating_add(1),
            _ => {}
        }
    }

    pub(crate) fn record_wire_reject(&mut self, reject: WireBatchReject) {
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

    pub(crate) fn report_if_due(&mut self, state: VisibleState) {
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
        // #region agent log — periodic inputd counter dump. MUST stay behind
        // this const gate: the `format!` allocates a ~600-byte String that the
        // non-freeing bump allocator never reclaims, so an ungated periodic dump
        // steadily exhausts inputd's heap (the resize-flood OOM crash). Off by
        // default; Phase 3 promotes these to metricsd counters (alloc-free).
        const INPUTD_FPS_TRACE: bool = false;
        if INPUTD_FPS_TRACE {
            let _ = debug_trace(&format!(
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
        }
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
