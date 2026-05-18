// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Input Method Editor daemon stub for TASK-0059 / RFC-0058.
//! OWNERS: @ui
//! STATUS: In Progress
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Unit tests
//! ADR: docs/rfcs/RFC-0058-ui-v3b-clip-scroll-effects-ime-contract.md

#![cfg_attr(all(nexus_env = "os", target_os = "none"), no_std)]
#![forbid(unsafe_code)]

extern crate alloc;

/// Which surface has keyboard/IME focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextFocus {
    pub surface_id: u64,
    pub focused: bool,
}

/// Caret position and selection range for a text input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CaretSelection {
    /// Caret position (byte offset into text content).
    pub caret_pos: usize,
    /// Selection anchor (start of selection). If equal to caret_pos, no selection.
    pub anchor: usize,
}

impl CaretSelection {
    pub const fn new(caret_pos: usize) -> Self {
        Self { caret_pos, anchor: caret_pos }
    }

    pub fn has_selection(&self) -> bool {
        self.caret_pos != self.anchor
    }

    pub fn selection_range(&self) -> core::ops::Range<usize> {
        if self.anchor <= self.caret_pos { self.anchor..self.caret_pos } else { self.caret_pos..self.anchor }
    }
}

/// IME daemon state.
#[derive(Debug, Clone)]
pub struct ImedService {
    pub ready: bool,
    pub focus: Option<TextFocus>,
    pub caret: CaretSelection,
}

impl ImedService {
    pub fn new() -> Self {
        Self { ready: true, focus: None, caret: CaretSelection::default() }
    }

    /// Set keyboard focus to a surface.
    pub fn set_focus(&mut self, surface_id: u64) {
        self.focus = Some(TextFocus { surface_id, focused: true });
    }

    /// Clear keyboard focus.
    pub fn clear_focus(&mut self) {
        self.focus = None;
    }

    /// Move caret by a delta in characters. Clamped to [0, text_len].
    pub fn move_caret(&mut self, text_len: usize, delta: i32) {
        let pos = self.caret.caret_pos as i32 + delta;
        self.caret.caret_pos = pos.max(0).min(text_len as i32) as usize;
        self.caret.anchor = self.caret.caret_pos;
    }

    /// Set selection range.
    pub fn set_selection(&mut self, anchor: usize, caret: usize, text_len: usize) {
        self.caret.anchor = anchor.min(text_len);
        self.caret.caret_pos = caret.min(text_len);
    }

    /// Ready marker string.
    pub const READY_MARKER: &'static str = "imed: ready";
}

impl Default for ImedService {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caret_selection_default_no_selection() {
        let cs = CaretSelection::default();
        assert!(!cs.has_selection());
        assert_eq!(cs.selection_range(), 0..0);
    }

    #[test]
    fn caret_selection_range_ordered() {
        let cs = CaretSelection { caret_pos: 5, anchor: 2 };
        assert!(cs.has_selection());
        assert_eq!(cs.selection_range(), 2..5);
    }

    #[test]
    fn caret_selection_range_reverse() {
        let cs = CaretSelection { caret_pos: 2, anchor: 5 };
        assert!(cs.has_selection());
        assert_eq!(cs.selection_range(), 2..5);
    }

    #[test]
    fn imed_service_ready_emits_marker() {
        let svc = ImedService::new();
        assert!(svc.ready);
        assert_eq!(ImedService::READY_MARKER, "imed: ready");
    }

    #[test]
    fn focus_routing_sets_and_clears() {
        let mut svc = ImedService::new();
        assert!(svc.focus.is_none());
        svc.set_focus(42);
        assert_eq!(svc.focus.unwrap().surface_id, 42);
        svc.clear_focus();
        assert!(svc.focus.is_none());
    }

    #[test]
    fn caret_movement_clamped() {
        let mut svc = ImedService::new();
        svc.move_caret(10, -5);
        assert_eq!(svc.caret.caret_pos, 0);
        svc.move_caret(10, 20);
        assert_eq!(svc.caret.caret_pos, 10);
    }
}
