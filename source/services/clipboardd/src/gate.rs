// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the clipboard gate — who may do what (RFC-0094 §Gate). Pure: the
//! kernel-attributed sender id, the op and windowd's last focus push decide.
//! Deny-by-default: sender id 0 is nobody, and before windowd's first push no
//! owner is known, so every read is refused.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: tests/contract.rs (`test_reject_*`)

/// The owners windowd last named (`OP_FOCUS`, retained latest-wins). 0 = none.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FocusTruth {
    /// Owner of the focused window (the paste target).
    pub focused: u64,
    /// Owner of the desktop surface — the shell (search, history).
    pub desktop: u64,
    /// Owner of the IME overlay — the on-screen keyboard (history, paste).
    pub ime: u64,
}

/// The access an op needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    /// Store an item: held by the route itself.
    Write,
    /// Read one item (the paste): focused window, shell or keyboard.
    Read,
    /// Browse or change the history: shell or keyboard only.
    History,
}

/// Whether `sender` may perform an op needing `access` under `truth`.
#[must_use]
pub fn allows(truth: &FocusTruth, sender: u64, access: Access) -> bool {
    if sender == 0 {
        return false;
    }
    let shell_or_keyboard = sender == truth.desktop || sender == truth.ime;
    match access {
        Access::Write => true,
        Access::Read => sender == truth.focused || shell_or_keyboard,
        Access::History => shell_or_keyboard,
    }
}

/// Whether a focus push from `sender` is windowd's (`windowd` = its kernel sid).
#[must_use]
pub fn is_focus_authority(sender: u64, windowd: u64) -> bool {
    sender != 0 && sender == windowd
}
