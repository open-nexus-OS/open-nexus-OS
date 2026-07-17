// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Telemetry observer — polls windowd for display/compose metrics.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Internal
//! TEST_COVERAGE: QEMU marker ladder
//!
//! Reads compose_hz, present_hz, frame latency, and visible input state
//! from the display pipeline without initiating control-plane IPC.

// RFC-0061 M4 pure-observer toolkit (telemetry-poller): declared observer API surface,
// kept per ADR-0027 until the observer ladder wires it in — module-scoped
// allow, not crate-level (repo rule).
#![allow(dead_code)]

use input_live_protocol::VisibleState;

/// Snapshot of observed display telemetry.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct DisplayTelemetry {
    /// Whether the backend framebuffer is visible.
    pub backend_visible: bool,
    /// Whether the first scanout completed.
    pub display_scanout_ready: bool,
    /// Whether SystemUI rendered its first frame.
    pub systemui_first_frame_visible: bool,
    /// Whether the scene is ready for interaction.
    pub scene_ready: bool,
    /// Compose rate in Hz (0 = not yet observed).
    pub compose_hz: u32,
    /// Present rate in Hz (0 = not yet observed).
    pub present_hz: u32,
    /// Latest visible input state received.
    pub visible_state: VisibleState,
}

impl DisplayTelemetry {
    /// Create a fresh telemetry snapshot.
    pub fn new() -> Self {
        Self::default()
    }

    /// Merge an observed visible state into this snapshot.
    /// Latches transient bits (they stick once observed).
    pub fn merge_visible_state(&mut self, state: VisibleState) {
        self.backend_visible |= state.backend_visible;
        self.display_scanout_ready |= state.display_scanout_ready;
        self.systemui_first_frame_visible |= state.systemui_first_frame_visible;
        self.scene_ready |= state.scene_ready;
        self.visible_state = state;
    }

    /// Returns true when the display path is fully initialized.
    pub fn display_ready(&self) -> bool {
        self.backend_visible && self.display_scanout_ready && self.systemui_first_frame_visible
    }

    /// Returns true when the scene is interactive.
    pub fn interactive_ready(&self) -> bool {
        self.scene_ready
            && self.visible_state.input_visible_on
            && self.visible_state.cursor_move_visible
    }
}
