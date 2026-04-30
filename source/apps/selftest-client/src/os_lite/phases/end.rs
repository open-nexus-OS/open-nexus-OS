// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Phase 12 of 12 — end (`SELFTEST: end` marker emission + cooperative
//!   idle loop; never returns).
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU marker ladders (`just test-os`, `just test-os visible-bootstrap`) — terminator and UI/v2a markers.
//!
//! Extracted in Cut P2-13 of TASK-0023B. Marker order and marker strings are
//! byte-identical to the pre-cut body. Returns `!` because the cooperative
//! idle loop never exits; this phase is always the last call in
//! `os_lite::run()`.
//!
//! ADR: docs/adr/0027-selftest-client-two-axis-architecture.md

use nexus_abi::yield_;

use crate::markers::{emit_bytes, emit_line, emit_u64};
use crate::os_lite::{context::PhaseCtx, display_bootstrap};

pub(crate) fn run(_ctx: &mut PhaseCtx) -> ! {
    if display_bootstrap::enabled() {
        if let Some(evidence) = display_bootstrap::run() {
            emit_line(windowd::DISPLAY_BOOTSTRAP_MARKER);
            emit_line(windowd::DISPLAY_MODE_MARKER);
            if evidence.systemui.backend_visible {
                emit_line(windowd::VISIBLE_BACKEND_MARKER);
            }
            emit_line(windowd::PRESENT_VISIBLE_MARKER);
            emit_line(windowd::DISPLAY_FIRST_SCANOUT_MARKER);
            if evidence.systemui.systemui_first_frame {
                emit_line(windowd::SYSTEMUI_FIRST_FRAME_VISIBLE_MARKER);
            }
            emit_line(windowd::SELFTEST_UI_VISIBLE_PRESENT_MARKER);
            if let Ok(v2a) = windowd::run_ui_v2a_smoke() {
                if v2a.present_scheduler_on {
                    emit_line(windowd::PRESENT_SCHEDULER_ON_MARKER);
                }
                if v2a.input_on {
                    emit_line(windowd::INPUT_ON_MARKER);
                    emit_bytes(b"windowd: focus -> ");
                    emit_u64(v2a.focused_surface.raw());
                    emit_bytes(b"\n");
                    if v2a.launcher_click_ok {
                        emit_line(windowd::LAUNCHER_CLICK_OK_MARKER);
                    }
                }
                if v2a.present_scheduler_on {
                    emit_line(windowd::SELFTEST_UI_V2_PRESENT_OK_MARKER);
                }
                if v2a.input_on && v2a.launcher_click_ok {
                    emit_line(windowd::SELFTEST_UI_V2_INPUT_OK_MARKER);
                }
            }
            let visible_input = evidence.visible_input;
            if visible_input.input_visible_on {
                emit_line(windowd::INPUT_VISIBLE_ON_MARKER);
            }
            if visible_input.cursor_move_visible {
                emit_line(windowd::CURSOR_MOVE_VISIBLE_MARKER);
            }
            if visible_input.hover_visible {
                emit_line(windowd::HOVER_VISIBLE_MARKER);
            }
            if visible_input.focus_visible {
                emit_line(windowd::FOCUS_VISIBLE_MARKER);
            }
            if visible_input.launcher_click_visible {
                emit_line(windowd::LAUNCHER_CLICK_VISIBLE_OK_MARKER);
            }
            if visible_input.input_visible_on
                && visible_input.cursor_move_visible
                && visible_input.hover_visible
                && visible_input.focus_visible
                && visible_input.launcher_click_visible
            {
                emit_line(windowd::SELFTEST_UI_VISIBLE_INPUT_OK_MARKER);
            }
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
