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

use nexus_abi::{nsec, yield_};

use crate::markers::{emit_bytes, emit_line, emit_u64};
use crate::os_lite::{context::PhaseCtx, display_bootstrap};
use crate::runtime_mode::RuntimeMode;

const INTERACTIVE_VISIBLE_STATE_POLL_INTERVAL_NS: u64 = 16_000_000;
const INTERACTIVE_VISIBLE_STATE_POLL_FALLBACK_TICKS: u32 = 64;

pub(crate) fn run(_ctx: &mut PhaseCtx) -> ! {
    let mut proof_completed = true;
    let mut interactive_mode = None;
    let mut interactive_input_visible_emitted = false;
    let mut interactive_cursor_visible_emitted = false;
    let mut interactive_hover_visible_emitted = false;
    let mut interactive_focus_visible_emitted = false;
    let mut interactive_click_visible_emitted = false;
    let mut interactive_keyboard_visible_emitted = false;
    let mut interactive_all_visible_emitted = false;
    if display_bootstrap::enabled() {
        let _ = nexus_abi::debug_println("dbg: end bootstrap enabled");
        proof_completed = false;
        if let Some(evidence) = display_bootstrap::run() {
            let _ = nexus_abi::debug_println("dbg: end bootstrap run ok");
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
            match evidence.runtime_mode {
                RuntimeMode::Proof => {
                    let _ = nexus_abi::debug_println("dbg: end bootstrap mode proof");
                    proof_completed = true;
                    emit_line(windowd::SELFTEST_UI_VISIBLE_PRESENT_MARKER);
                    if let Some(proof) = evidence.proof {
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
                        let visible_input = proof.visible_state;
                        if visible_input.input_visible_on {
                            emit_line(windowd::INPUT_VISIBLE_ON_MARKER);
                        }
                        if visible_input.full_window_visible {
                            emit_line(windowd::FULL_WINDOW_VISIBLE_MARKER);
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
                        if visible_input.keyboard_visible {
                            emit_line(windowd::KEYBOARD_VISIBLE_MARKER);
                        }
                        if visible_input.input_visible_on
                            && visible_input.full_window_visible
                            && visible_input.cursor_move_visible
                            && visible_input.hover_visible
                            && visible_input.focus_visible
                            && visible_input.launcher_click_visible
                            && visible_input.keyboard_visible
                        {
                            emit_line(windowd::SELFTEST_UI_VISIBLE_INPUT_OK_MARKER);
                        }
                    }
                }
                RuntimeMode::InteractiveMinimal => {
                    let _ = nexus_abi::debug_println("dbg: end bootstrap mode interactive-minimal");
                    proof_completed = false;
                    interactive_mode = Some(RuntimeMode::InteractiveMinimal);
                    if let Some(interactive) = evidence.interactive {
                        if interactive.scene_ready && interactive.full_window_visible {
                            emit_line(windowd::INTERACTIVE_SCENE_READY_MARKER);
                        }
                    }
                    let _ = nexus_abi::debug_println(
                        "debug8cde1d: interactive minimal live-route observer",
                    );
                }
                RuntimeMode::InteractiveFull => {
                    let _ = nexus_abi::debug_println("dbg: end bootstrap mode interactive-full");
                    proof_completed = false;
                    interactive_mode = Some(RuntimeMode::InteractiveFull);
                    if let Some(interactive) = evidence.interactive {
                        if interactive.scene_ready && interactive.full_window_visible {
                            emit_line(windowd::INTERACTIVE_SCENE_READY_MARKER);
                            emit_line(windowd::INTERACTIVE_FULL_MARKERS_MARKER);
                        }
                        if interactive.click_target_visible {
                            emit_line(windowd::INTERACTIVE_CLICK_TARGET_READY_MARKER);
                        }
                        if interactive.keyboard_target_visible {
                            emit_line(windowd::INTERACTIVE_KEYBOARD_TARGET_READY_MARKER);
                        }
                    }
                    let _ = nexus_abi::debug_println(
                        "debug8cde1d: interactive full live-route observer",
                    );
                }
            }
        } else {
            let _ = nexus_abi::debug_println("dbg: end bootstrap run none");
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

    if proof_completed {
        emit_line(crate::markers::M_SELFTEST_END);
    }

    // Stay alive (cooperative).
    let mut idle_ticks: u32 = 0;
    let mut last_interactive_poll_ns = nsec().unwrap_or(0);
    let mut last_interactive_poll_tick = 0u32;
    loop {
        idle_ticks = idle_ticks.wrapping_add(1);
        if matches!(
            interactive_mode,
            Some(RuntimeMode::InteractiveMinimal | RuntimeMode::InteractiveFull)
        ) && should_poll_interactive_visible_state(
            idle_ticks,
            &mut last_interactive_poll_tick,
            &mut last_interactive_poll_ns,
        ) {
            if let Some(state) = display_bootstrap::interactive_live_tick() {
                if interactive_mode == Some(RuntimeMode::InteractiveFull) {
                    if state.input_visible_on && !interactive_input_visible_emitted {
                        emit_line(windowd::INPUT_VISIBLE_ON_MARKER);
                        interactive_input_visible_emitted = true;
                    }
                    if state.cursor_move_visible && !interactive_cursor_visible_emitted {
                        emit_line(windowd::CURSOR_MOVE_VISIBLE_MARKER);
                        interactive_cursor_visible_emitted = true;
                    }
                    if state.hover_visible && !interactive_hover_visible_emitted {
                        emit_line(windowd::HOVER_VISIBLE_MARKER);
                        interactive_hover_visible_emitted = true;
                    }
                    if state.focus_visible && !interactive_focus_visible_emitted {
                        emit_line(windowd::FOCUS_VISIBLE_MARKER);
                        interactive_focus_visible_emitted = true;
                    }
                    if state.launcher_click_visible && !interactive_click_visible_emitted {
                        emit_line(windowd::LAUNCHER_CLICK_VISIBLE_OK_MARKER);
                        interactive_click_visible_emitted = true;
                    }
                    if state.keyboard_visible && !interactive_keyboard_visible_emitted {
                        emit_line(windowd::KEYBOARD_VISIBLE_MARKER);
                        interactive_keyboard_visible_emitted = true;
                    }
                    if state.input_visible_on
                        && state.full_window_visible
                        && state.cursor_move_visible
                        && state.hover_visible
                        && state.focus_visible
                        && state.launcher_click_visible
                        && state.keyboard_visible
                        && !interactive_all_visible_emitted
                    {
                        emit_line(windowd::SELFTEST_UI_VISIBLE_INPUT_OK_MARKER);
                        interactive_all_visible_emitted = true;
                    }
                }
            }
        }
        let _ = yield_();
    }
}

fn should_poll_interactive_visible_state(
    idle_ticks: u32,
    last_poll_tick: &mut u32,
    last_poll_ns: &mut u64,
) -> bool {
    let now_ns = nsec().unwrap_or(0);
    if now_ns != 0 {
        if *last_poll_ns == 0 {
            *last_poll_ns = now_ns;
            return true;
        }
        if now_ns.saturating_sub(*last_poll_ns) >= INTERACTIVE_VISIBLE_STATE_POLL_INTERVAL_NS {
            *last_poll_ns = now_ns;
            *last_poll_tick = idle_ticks;
            return true;
        }
        return false;
    }

    if idle_ticks.wrapping_sub(*last_poll_tick) >= INTERACTIVE_VISIBLE_STATE_POLL_FALLBACK_TICKS {
        *last_poll_tick = idle_ticks;
        return true;
    }
    false
}
