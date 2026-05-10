// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Minimal SystemUI IME overlay hook state for TASK-0253.
//! OWNERS: @ui @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `cargo test -p systemui -- --nocapture`
//! ADR: docs/adr/0028-windowd-surface-present-and-visible-bootstrap-architecture.md

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImeOverlayState {
    visible: bool,
    show_events: u32,
    hide_events: u32,
}

impl ImeOverlayState {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            visible: false,
            show_events: 0,
            hide_events: 0,
        }
    }

    pub fn show(&mut self) -> bool {
        if self.visible {
            return false;
        }
        self.visible = true;
        self.show_events += 1;
        true
    }

    pub fn hide(&mut self) -> bool {
        if !self.visible {
            return false;
        }
        self.visible = false;
        self.hide_events += 1;
        true
    }

    #[must_use]
    pub const fn visible(self) -> bool {
        self.visible
    }

    #[must_use]
    pub const fn show_events(self) -> u32 {
        self.show_events
    }

    #[must_use]
    pub const fn hide_events(self) -> u32 {
        self.hide_events
    }
}

impl Default for ImeOverlayState {
    fn default() -> Self {
        Self::new()
    }
}
