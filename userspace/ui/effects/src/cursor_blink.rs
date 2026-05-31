// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Cursor blink timer for TextInput caret rendering (TASK-0059 / RFC-0058).
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 2 tests (tests/ui_v3b_host/ — cursor blink toggle/reset)
//! ADR: docs/rfcs/RFC-0058-ui-v3b-clip-scroll-effects-ime-contract.md
//! Cursor blink timer for TextInput caret rendering.
//! Blinks at 500ms intervals (configurable), deterministic (frame-count based).

/// Cursor blink state — visible/hidden toggled at regular intervals.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CursorBlink {
    pub visible: bool,
    tick_interval: u64,
    current_tick: u64,
}

impl CursorBlink {
    /// Default blink interval: 500ms. At 60 Hz, that's 30 frames.
    pub const DEFAULT_INTERVAL: u64 = 30;

    pub fn new() -> Self {
        Self { visible: true, tick_interval: Self::DEFAULT_INTERVAL, current_tick: 0 }
    }

    /// Set the blink interval in frame ticks.
    pub fn with_interval(interval: u64) -> Self {
        Self { visible: true, tick_interval: interval, current_tick: 0 }
    }

    /// Advance the blink timer by one frame tick. Toggles visibility when threshold reached.
    pub fn tick(&mut self) {
        self.current_tick += 1;
        if self.current_tick >= self.tick_interval {
            self.current_tick = 0;
            self.visible = !self.visible;
        }
    }

    /// Reset to visible state.
    pub fn reset(&mut self) {
        self.current_tick = 0;
        self.visible = true;
    }
}

impl Default for CursorBlink {
    fn default() -> Self {
        Self::new()
    }
}
