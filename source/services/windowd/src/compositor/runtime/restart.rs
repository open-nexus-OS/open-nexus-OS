// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: windowd's restart (RFC-0112, amendment 2026-11-02; TASK-0348 Phase 6). The user's
//! "restart the desktop" chord (`zones::CODE_RESTART`, the shortcut registry's
//! `window.restartDesktop`) ends this process — a fault takes the same way. init's supervision
//! respawns windowd from the sealed image and re-pins its clients; execd tells every app, and
//! each app attaches again and draws.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the restart lane (`windowd: restart requested (chord)`, `init: service
//!   restarted name=windowd`)

use super::*;

/// The exit status a requested restart ends with (EX_TEMPFAIL, "try again"): init's supervision
/// respawns on it at once and counts it toward no crash window (TASK-0348 P11) — the chord may
/// be pressed as often as the desktop is wanted back.
const RESTART_STATUS: i32 = nexus_abi::EXIT_RESTART_REQUESTED;

impl DisplayServerRuntime {
    /// The restart chord: windowd ends itself, init brings it back (RFC-0112).
    pub(super) fn restart_requested(&mut self) -> ! {
        let _ = debug_println("windowd: restart requested (chord)");
        nexus_abi::exit(RESTART_STATUS)
    }
}
