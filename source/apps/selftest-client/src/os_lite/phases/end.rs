// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Phase 12 of 12 — end (`SELFTEST: end` marker emission + cooperative
//!   idle loop; never returns).
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU marker ladder (just test-os) — terminator marker.
//!
//! Extracted in Cut P2-13 of TASK-0023B. Marker order and marker strings are
//! byte-identical to the pre-cut body. Returns `!` because the cooperative
//! idle loop never exits; this phase is always the last call in
//! `os_lite::run()`.
//!
//! ADR: docs/adr/0027-selftest-client-two-axis-architecture.md

use nexus_abi::yield_;

use crate::markers::emit_line;
use crate::os_lite::{context::PhaseCtx, display_bootstrap};

pub(crate) fn run(_ctx: &mut PhaseCtx) -> ! {
    if display_bootstrap::enabled() {
        if let Some(evidence) = display_bootstrap::run() {
            emit_line(windowd::DISPLAY_BOOTSTRAP_MARKER);
            emit_line(windowd::DISPLAY_MODE_MARKER);
            let present_marker = windowd::present_marker(evidence.first_present);
            emit_line(present_marker.as_str());
            emit_line(windowd::DISPLAY_FIRST_SCANOUT_MARKER);
            emit_line(windowd::SELFTEST_DISPLAY_BOOTSTRAP_VISIBLE_MARKER);
        }
    } else if let Ok(evidence) = windowd::run_headless_ui_smoke() {
        if evidence.ready {
            emit_line(windowd::READY_MARKER);
        }
        if evidence.systemui_loaded {
            emit_line(windowd::SYSTEMUI_MARKER);
        }
        if evidence.launcher_first_frame {
            let present_marker = windowd::present_marker(evidence.first_present);
            emit_line(present_marker.as_str());
            emit_line(windowd::LAUNCHER_MARKER);
            emit_line(windowd::SELFTEST_LAUNCHER_PRESENT_MARKER);
        }
        if evidence.resize_ok {
            emit_line(windowd::SELFTEST_RESIZE_MARKER);
        }
    }

    emit_line(crate::markers::M_SELFTEST_END);

    // Stay alive (cooperative).
    loop {
        let _ = yield_();
    }
}
