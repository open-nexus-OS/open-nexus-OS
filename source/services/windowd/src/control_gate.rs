// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: RFC-0086 sender-identity gates for window verbs — pure decisions,
//! host-tested (`compositor/` is OS-glue). Identity = `sender_service_id`
//! from the kernel IPC recv, NEVER a payload byte; sid 0 (an unnamed spawn,
//! or a slot whose owner was never captured) is ALWAYS refused — deny by
//! default, presentation stays fail-closed.
//!
//! Two gates, one shape:
//!   * `own_window_gate` — an app's `CONTROL_WIN_*` may act on its OWN
//!     window only (closes the recorded spoofable-value-byte follow-up).
//!   * `taskbar_gate` — `OP_SURFACE_TASKBAR` verbs are accepted from the
//!     DESKTOP-surface owner (the shell) only.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: reject-table tests below (`test_reject_*`).

/// Verdict of an identity gate. `Allow` is the only actionable outcome; the
/// reject variants exist so call sites can emit DISTINCT markers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateVerdict {
    Allow,
    /// The sender (or the gated owner slot) carries no identity (sid 0).
    RejectSidZero,
    /// Real identities that do not match — a foreign caller.
    RejectForeign,
}

/// May `sender_sid` act on a window owned by `owner_sid`?
#[must_use]
pub fn own_window_gate(owner_sid: u64, sender_sid: u64) -> GateVerdict {
    if sender_sid == 0 || owner_sid == 0 {
        GateVerdict::RejectSidZero
    } else if sender_sid != owner_sid {
        GateVerdict::RejectForeign
    } else {
        GateVerdict::Allow
    }
}

/// May `sender_sid` send taskbar verbs, given the captured desktop owner?
/// Same law: both identities must be real AND equal.
#[must_use]
pub fn taskbar_gate(desktop_owner_sid: u64, sender_sid: u64) -> GateVerdict {
    own_window_gate(desktop_owner_sid, sender_sid)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHELL: u64 = 0xA11C_E555;
    const APP_A: u64 = 0xB0B0_0001;
    const APP_B: u64 = 0xB0B0_0002;

    #[test]
    fn own_window_allows_the_owner() {
        assert_eq!(own_window_gate(APP_A, APP_A), GateVerdict::Allow);
    }

    #[test]
    fn test_reject_win_control_foreign_surface() {
        // App B naming app A's surface id in the value byte: the sid gate
        // refuses regardless of what the payload claims.
        assert_eq!(own_window_gate(APP_A, APP_B), GateVerdict::RejectForeign);
    }

    #[test]
    fn test_reject_win_control_sid_zero() {
        // Unnamed sender (pre-identity spawn) — refused.
        assert_eq!(own_window_gate(APP_A, 0), GateVerdict::RejectSidZero);
        // Owner never captured — refused even for a named sender: a slot
        // without identity must not be actionable by anyone.
        assert_eq!(own_window_gate(0, APP_A), GateVerdict::RejectSidZero);
        assert_eq!(own_window_gate(0, 0), GateVerdict::RejectSidZero);
    }

    #[test]
    fn test_reject_taskbar_verb_from_non_desktop_owner() {
        // An ordinary app-host forging ACTIVATE: refused.
        assert_eq!(taskbar_gate(SHELL, APP_A), GateVerdict::RejectForeign);
    }

    #[test]
    fn test_reject_taskbar_verb_sid_zero() {
        assert_eq!(taskbar_gate(SHELL, 0), GateVerdict::RejectSidZero);
        // No desktop surface ever bound (owner 0): everything refused.
        assert_eq!(taskbar_gate(0, SHELL), GateVerdict::RejectSidZero);
    }

    #[test]
    fn taskbar_allows_the_desktop_owner() {
        assert_eq!(taskbar_gate(SHELL, SHELL), GateVerdict::Allow);
    }
}
