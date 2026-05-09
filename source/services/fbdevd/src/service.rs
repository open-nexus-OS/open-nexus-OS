// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Service-owned display state cache and display/input merge logic for `fbdevd`.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `fbdevd` host reject tests.
//! ADR: docs/adr/0028-windowd-surface-present-and-visible-bootstrap-architecture.md

use alloc::string::String;

use input_live_protocol::VisibleState;
use windowd::{DisplayPresentHandoff, WindowdDisplayTelemetry};

use crate::error::Result;
use crate::protocol::{display_ready_for_observer, merge_visible_state};
use crate::scanout::DisplayScanout;

#[derive(Debug, Clone)]
pub struct FbdevService {
    state: VisibleState,
    generation: u64,
    display_enabled: bool,
    scanout: DisplayScanout,
    windowd_telemetry: WindowdDisplayTelemetry,
}

impl FbdevService {
    pub fn disabled() -> Self {
        Self {
            state: VisibleState::default(),
            generation: 0,
            display_enabled: false,
            scanout: DisplayScanout::new(),
            windowd_telemetry: WindowdDisplayTelemetry::default(),
        }
    }

    pub fn enabled(bootstrap: &DisplayPresentHandoff) -> Result<Self> {
        let mut scanout = DisplayScanout::new();
        scanout.configure();
        let mut service = Self {
            state: VisibleState {
                backend_visible: bootstrap.backend_visible,
                display_scanout_ready: bootstrap.scanout_ready,
                systemui_first_frame_visible: bootstrap.systemui_first_frame_visible,
                ..VisibleState::default()
            },
            generation: 0,
            display_enabled: true,
            scanout,
            windowd_telemetry: WindowdDisplayTelemetry::default(),
        };
        let _ = service.present(bootstrap)?;
        Ok(service)
    }

    pub const fn display_enabled(&self) -> bool {
        self.display_enabled
    }

    pub const fn visible_state(&self) -> VisibleState {
        self.state
    }

    pub fn observer_ready(&self) -> bool {
        display_ready_for_observer(self.state)
    }

    pub fn merge_input_state(&mut self, upstream: VisibleState) {
        self.state = merge_visible_state(
            self.state,
            upstream,
            self.state.backend_visible,
            self.state.display_scanout_ready,
            self.state.systemui_first_frame_visible,
        );
    }

    pub fn present(&mut self, handoff: &DisplayPresentHandoff) -> Result<u64> {
        self.generation = self.generation.saturating_add(1);
        self.windowd_telemetry
            .record_compose(u64::from(handoff.mode.width) * u64::from(handoff.mode.height));
        self.windowd_telemetry.record_present();
        self.scanout.present(self.generation, handoff)
    }

    pub fn present_live_bytes(&mut self, byte_len: usize) -> Result<u64> {
        self.generation = self.generation.saturating_add(1);
        self.windowd_telemetry.record_compose((byte_len / 4) as u64);
        self.windowd_telemetry.record_present();
        self.scanout.present_bytes(self.generation, byte_len)
    }

    pub fn telemetry_if_due(&mut self, now_ns: u64) -> Option<(String, String)> {
        let windowd = self.windowd_telemetry.report_if_due(now_ns);
        let fbdevd = self.scanout.report_if_due(now_ns);
        match (windowd, fbdevd) {
            (None, None) => None,
            (Some(windowd), Some(fbdevd)) => Some((windowd, fbdevd)),
            (Some(windowd), None) => Some((windowd, String::new())),
            (None, Some(fbdevd)) => Some((String::new(), fbdevd)),
        }
    }
}
