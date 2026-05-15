// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Small telemetry helper for service-owned display composition stats.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Indirectly exercised by `fbdevd` host tests and QEMU traces.
//! ADR: docs/adr/0028-windowd-surface-present-and-visible-bootstrap-architecture.md

use alloc::{format, string::String};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WindowdDisplayTelemetry {
    last_report_ns: u64,
    compose_events: u64,
    present_events: u64,
    coalesced_events: u64,
    dropped_events: u64,
    damage_pixels: u64,
}

impl WindowdDisplayTelemetry {
    pub const REPORT_INTERVAL_NS: u64 = 1_000_000_000;

    pub fn record_compose(&mut self, damage_pixels: u64) {
        self.compose_events = self.compose_events.saturating_add(1);
        self.damage_pixels = self.damage_pixels.saturating_add(damage_pixels);
    }

    pub fn record_present(&mut self) {
        self.present_events = self.present_events.saturating_add(1);
    }

    pub fn record_coalesced(&mut self) {
        self.coalesced_events = self.coalesced_events.saturating_add(1);
    }

    pub fn record_drop(&mut self) {
        self.dropped_events = self.dropped_events.saturating_add(1);
    }

    pub fn report_if_due(&mut self, now_ns: u64) -> Option<String> {
        if now_ns == 0 {
            return None;
        }
        if self.last_report_ns == 0 {
            self.last_report_ns = now_ns;
            return None;
        }
        let elapsed = now_ns.saturating_sub(self.last_report_ns);
        if elapsed < Self::REPORT_INTERVAL_NS {
            return None;
        }
        let compose_hz =
            self.compose_events.saturating_mul(1_000_000_000).checked_div(elapsed).unwrap_or(0);
        let present_hz =
            self.present_events.saturating_mul(1_000_000_000).checked_div(elapsed).unwrap_or(0);
        let line = format!(
            "fps: windowd compose_hz={} present_hz={} coalesced={} dropped={} damage_px={}",
            compose_hz, present_hz, self.coalesced_events, self.dropped_events, self.damage_pixels
        );
        self.last_report_ns = now_ns;
        self.compose_events = 0;
        self.present_events = 0;
        self.coalesced_events = 0;
        self.dropped_events = 0;
        self.damage_pixels = 0;
        Some(line)
    }
}
