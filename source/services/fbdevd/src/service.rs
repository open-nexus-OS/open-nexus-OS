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
use windowd::{DisplayPresentHandoff, WindowdDisplayTelemetry, WindowdDisplayTelemetryReport};

use crate::error::Result;
use crate::protocol::{
    display_ready_for_observer, merge_observer_visible_state, merge_visible_state,
};
use crate::scanout::{DisplayScanout, DisplayScanoutReport};

#[derive(Debug, Clone)]
pub struct FbdevService {
    render_state: VisibleState,
    observer_state: VisibleState,
    generation: u64,
    display_enabled: bool,
    scanout: DisplayScanout,
    windowd_telemetry: WindowdDisplayTelemetry,
    /// Cursor BGRA8888 bitmap from windowd bootstrap (None if not loaded).
    cursor_bitmap: Option<alloc::vec::Vec<u8>>,
    /// Cursor width in pixels.
    cursor_width: u32,
    /// Cursor height in pixels.
    cursor_height: u32,
}

impl FbdevService {
    pub fn disabled() -> Self {
        Self {
            render_state: VisibleState::default(),
            observer_state: VisibleState::default(),
            generation: 0,
            display_enabled: false,
            scanout: DisplayScanout::new(),
            windowd_telemetry: WindowdDisplayTelemetry::default(),
            cursor_bitmap: None,
            cursor_width: 0,
            cursor_height: 0,
        }
    }

    pub fn enabled(bootstrap: &DisplayPresentHandoff) -> Result<Self> {
        let mut scanout = DisplayScanout::new();
        scanout.configure();
        let initial_state = VisibleState {
            backend_visible: bootstrap.backend_visible,
            display_scanout_ready: bootstrap.scanout_ready,
            systemui_first_frame_visible: bootstrap.systemui_first_frame_visible,
            ..VisibleState::default()
        };
        let mut service = Self {
            render_state: initial_state,
            observer_state: initial_state,
            generation: 0,
            display_enabled: true,
            scanout,
            windowd_telemetry: WindowdDisplayTelemetry::default(),
            cursor_bitmap: bootstrap.cursor_bitmap.clone(),
            cursor_width: bootstrap.cursor_width,
            cursor_height: bootstrap.cursor_height,
        };
        let _ = service.present(bootstrap)?;
        Ok(service)
    }

    pub const fn display_enabled(&self) -> bool {
        self.display_enabled
    }

    /// Enable display for scanout-only operation (windowd composes frames).
    pub fn set_display_enabled(&mut self, enabled: bool) {
        self.display_enabled = enabled;
        if enabled {
            self.render_state.backend_visible = true;
            self.render_state.display_scanout_ready = true;
            self.observer_state.backend_visible = true;
            self.observer_state.display_scanout_ready = true;
            self.scanout.configure();
        }
    }

    pub const fn visible_state(&self) -> VisibleState {
        self.observer_state
    }

    pub const fn render_state(&self) -> VisibleState {
        self.render_state
    }

    /// Returns the cursor bitmap and dimensions, if loaded.
    pub fn cursor_overlay(&self) -> Option<(&[u8], u32, u32)> {
        self.cursor_bitmap.as_ref().map(|bm| (bm.as_slice(), self.cursor_width, self.cursor_height))
    }

    pub fn observer_ready(&self) -> bool {
        display_ready_for_observer(self.observer_state)
    }

    pub fn merge_input_state(&mut self, upstream: VisibleState) {
        self.render_state = merge_visible_state(
            self.render_state,
            upstream,
            self.render_state.backend_visible,
            self.render_state.display_scanout_ready,
            self.render_state.systemui_first_frame_visible,
        );
        self.observer_state = merge_observer_visible_state(
            self.observer_state,
            upstream,
            self.observer_state.backend_visible,
            self.observer_state.display_scanout_ready,
            self.observer_state.systemui_first_frame_visible,
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

    pub fn telemetry_values_if_due(
        &mut self,
        now_ns: u64,
    ) -> Option<(Option<WindowdDisplayTelemetryReport>, Option<DisplayScanoutReport>)> {
        let windowd = self.windowd_telemetry.report_values_if_due(now_ns);
        let fbdevd = self.scanout.report_values_if_due(now_ns);
        if windowd.is_none() && fbdevd.is_none() {
            None
        } else {
            Some((windowd, fbdevd))
        }
    }
}
