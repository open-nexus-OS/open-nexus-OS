// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Text and action outputs emitted by deterministic base keymap resolution.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: No direct tests (covered by 4 integration tests in `tests/input_v1_0_host/tests/keymaps_contract.rs`).
//! ADR: docs/adr/0029-input-v1-host-core-architecture.md

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyAction {
    Enter,
    Escape,
    Backspace,
    Tab,
    /// Shift+Tab (TASK-0346 P2): the focus goes to the field before.
    BackTab,
    ImeSwitch,
    /// A text-field editing command (TASK-0067B): a field chord of the shortcut registry — the
    /// navigation keys (Shift selects), by words with Ctrl, the Ctrl letters by the layout's
    /// letter. Composition never consumes it — imed passes it to the focused field.
    Edit(EditKey),
}

/// The editing commands a keyboard sends a focused text field — the shortcut registry's
/// vocabulary (RFC-0113 §7), re-exported where the keymap's outputs live.
pub use shortcuts::EditKey;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyOutput {
    Text(char),
    /// Dead key: does not emit text by itself; the IME compose machine
    /// (ime-core, RFC-0075) combines it with the next Text key.
    Dead(char),
    Action(KeyAction),
}
