// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: hidrawd ingest telemetry — the folded `fps:` chain-counter dump
//! plus the plain (unfolded) `hidrawd: wake/rx/ev/tx hz` rate line for
//! input-rate triage. Split out of `os_lite.rs` (structure-gate).

#![cfg(all(feature = "os-lite", nexus_env = "os", target_os = "none"))]

extern crate alloc;

use alloc::format;
use nexus_abi::{debug_println, nsec};

pub(crate) struct HidrawChainTelemetry {
    pub(crate) last_report_ns: u64,
    pub(crate) rate_window_ns: u64,
    pub(crate) wake_rate_count: u32,
    pub(crate) rx_rate_count: u32,
    pub(crate) ev_rate_count: u32,
    pub(crate) tx_rate_count: u32,
    pub(crate) raw_batches: u64,
    pub(crate) wire_batches: u64,
    pub(crate) wire_batches_skipped: u64,
    pub(crate) sent_batches: u64,
    pub(crate) raw_events: u64,
    pub(crate) normalized_events: u64,
    pub(crate) keyboard_batches: u64,
    pub(crate) mouse_relative_batches: u64,
    pub(crate) tablet_absolute_batches: u64,
    pub(crate) touch_absolute_batches: u64,
    pub(crate) send_failures: u64,
    pub(crate) route_rebinds: u64,
    pub(crate) idle_yields: u64,
}

impl HidrawChainTelemetry {
    const REPORT_INTERVAL_NS: u64 = 1_000_000_000;

    pub(crate) fn new() -> Self {
        Self {
            last_report_ns: nsec().unwrap_or(0),
            rate_window_ns: 0,
            wake_rate_count: 0,
            rx_rate_count: 0,
            ev_rate_count: 0,
            tx_rate_count: 0,
            raw_batches: 0,
            wire_batches: 0,
            wire_batches_skipped: 0,
            sent_batches: 0,
            raw_events: 0,
            normalized_events: 0,
            keyboard_batches: 0,
            mouse_relative_batches: 0,
            tablet_absolute_batches: 0,
            touch_absolute_batches: 0,
            send_failures: 0,
            route_rebinds: 0,
            idle_yields: 0,
        }
    }

    /// Plain (unfolded) rate lines, >=8/s gate — the input-rate triage
    /// counterparts of `inputd: push hz` / `windowd: loop hz` (the folded
    /// `fps:` dump is invisible in interactive boots). One combined line:
    /// `wake` = loop passes (IRQ/park wakes), `rx` = raw batches, `ev` = raw
    /// events, `tx` = wire batches sent to inputd. wake high + rx low = ring/
    /// device dry; wake low under a storm = IRQ starvation; rx high + tx low =
    /// wire-path loss.
    pub(crate) fn note_wake_for_rate_line(&mut self) {
        self.wake_rate_count = self.wake_rate_count.saturating_add(1);
        let now_ns = nsec().unwrap_or(0);
        if self.rate_window_ns == 0 {
            self.rate_window_ns = now_ns;
        } else if now_ns.saturating_sub(self.rate_window_ns) >= 1_000_000_000 {
            if self.wake_rate_count > 0 {
                let _ = debug_println(&format!(
                    "hidrawd: wake hz={} rx hz={} ev hz={} tx hz={}",
                    self.wake_rate_count,
                    self.rx_rate_count,
                    self.ev_rate_count,
                    self.tx_rate_count
                ));
            }
            self.rate_window_ns = now_ns;
            self.wake_rate_count = 0;
            self.rx_rate_count = 0;
            self.ev_rate_count = 0;
            self.tx_rate_count = 0;
        }
    }

    pub(crate) fn note_rx_for_rate_line(&mut self, raw_events: u32) {
        self.rx_rate_count = self.rx_rate_count.saturating_add(1);
        self.ev_rate_count = self.ev_rate_count.saturating_add(raw_events);
    }

    pub(crate) fn note_tx_for_rate_line(&mut self) {
        self.tx_rate_count = self.tx_rate_count.saturating_add(1);
    }

    pub(crate) fn report_if_due(&mut self) {
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
        let ingress_hz =
            self.raw_batches.saturating_mul(1_000_000_000).checked_div(elapsed).unwrap_or(0);
        let sent_hz =
            self.sent_batches.saturating_mul(1_000_000_000).checked_div(elapsed).unwrap_or(0);
        // #region agent log — RFC-0068: the 30-field fps counter dump folds in interactive boots
        // (recall `NEXUS_LOG_EXPAND=hidrawd`) and prints raw in proof. Gated via `service_trace`
        // directly (NOT trace_line) because the `send_fail=` field would trip the failure safety net.
        let _ = (!nexus_abi::service_trace()).then(|| debug_println(&format!(
            "fps: hidrawd ingress_hz={} sent_hz={} raw_batches={} wire_batches={} wire_skip={} raw_events={} norm_events={} kbd_batches={} mouse_rel={} tablet_abs={} touch_abs={} send_fail={} rebinds={} idle_yields={}",
            ingress_hz,
            sent_hz,
            self.raw_batches,
            self.wire_batches,
            self.wire_batches_skipped,
            self.raw_events,
            self.normalized_events,
            self.keyboard_batches,
            self.mouse_relative_batches,
            self.tablet_absolute_batches,
            self.touch_absolute_batches,
            self.send_failures,
            self.route_rebinds,
            self.idle_yields
        )));
        // #endregion
        self.last_report_ns = now_ns;
        self.raw_batches = 0;
        self.wire_batches = 0;
        self.wire_batches_skipped = 0;
        self.sent_batches = 0;
        self.raw_events = 0;
        self.normalized_events = 0;
        self.keyboard_batches = 0;
        self.mouse_relative_batches = 0;
        self.tablet_absolute_batches = 0;
        self.touch_absolute_batches = 0;
        self.send_failures = 0;
        self.route_rebinds = 0;
        self.idle_yields = 0;
    }
}
