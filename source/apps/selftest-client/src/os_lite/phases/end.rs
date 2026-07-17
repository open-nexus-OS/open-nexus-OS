// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Phase 12 of 12 — end (proof-mode termination or cooperative
//!   idle loop; never returns). Terminal verification is handled by the
//!   host-side harness (scripts/qemu-test.sh) which emits
//!   `SELFTEST: Completed (markers verified)` after checking all markers.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU marker ladders (`just test-os`) — terminator and UI/v2a markers.
//!
//! Extracted in Cut P2-13 of TASK-0023B. Marker order and marker strings are
//! byte-identical to the pre-cut body. Returns `!` because proof mode may exit
//! the current task and interactive mode falls back to the cooperative idle loop;
//! this phase is always the last call in
//! `os_lite::run()`.
//!
//! ADR: docs/adr/0027-selftest-client-two-axis-architecture.md

use nexus_abi::{exit, nsec, yield_};

use input_live_protocol::VisibleState;

use crate::markers::{emit_bytes, emit_line, emit_u64};
use crate::os_lite::{
    context::PhaseCtx,
    display_bootstrap,
    display_observer::{proof_v2b_assets_ready, ProofVisibleInputWitness},
};
use crate::runtime_mode::RuntimeMode;

const INTERACTIVE_VISIBLE_STATE_POLL_INTERVAL_NS: u64 = 16_000_000;
const INTERACTIVE_VISIBLE_STATE_POLL_FALLBACK_TICKS: u32 = 64;
const V2A_SMOKE_ERR_MARKER: &str = "windowd: v2a smoke err";
const V2A_SCHEDULER_OFF_MARKER: &str = "windowd: v2a scheduler off";
const V2A_INPUT_OFF_MARKER: &str = "windowd: v2a input off";
const V2A_CLICK_OFF_MARKER: &str = "windowd: v2a click off";

pub(crate) fn run(_ctx: &mut PhaseCtx) -> ! {
    // P0: pull the kernel's BKL budget gate line NOW — the ladder is done, so
    // the report covers the full bring-up contention window.
    nexus_abi::sched::bkl_budget_report();
    let auto_exit_after_proof = display_bootstrap::enabled()
        && crate::os_lite::boot_cfg::runtime_mode_with_retry().unwrap_or(RuntimeMode::Proof)
            == RuntimeMode::Proof;
    let mut proof_completed = true;
    let mut proof_witness = ProofVisibleInputWitness::new();
    let mut interactive_mode = None;
    let mut visible_present_marker_emitted = false;
    let mut interactive_input_visible_emitted = false;
    let mut interactive_cursor_visible_emitted = false;
    let mut interactive_hover_visible_emitted = false;
    let mut interactive_focus_visible_emitted = false;
    let mut interactive_click_visible_emitted = false;
    let mut interactive_keyboard_visible_emitted = false;
    let mut interactive_all_visible_emitted = false;
    if display_bootstrap::enabled() {
        proof_completed = false;
        if let Ok(display) = display_bootstrap::observe_display_evidence() {
            emit_line(windowd::DISPLAY_BOOTSTRAP_MARKER);
            emit_line(windowd::DISPLAY_MODE_MARKER);
            if display.backend_visible {
                emit_line(windowd::VISIBLE_BACKEND_MARKER);
            }
            emit_line(windowd::PRESENT_VISIBLE_MARKER);
            if display.first_scanout_ready {
                emit_line(windowd::DISPLAY_FIRST_SCANOUT_MARKER);
            }
            if display.systemui_first_frame {
                emit_line(windowd::SYSTEMUI_FIRST_FRAME_VISIBLE_MARKER);
            }
            emit_line(windowd::SELFTEST_UI_VISIBLE_PRESENT_MARKER);
            visible_present_marker_emitted = true;
        }
        if let Some(evidence) = display_bootstrap::run() {
            emit_line(windowd::DISPLAY_BOOTSTRAP_MARKER);
            emit_line(windowd::DISPLAY_MODE_MARKER);
            if evidence.display.backend_visible {
                emit_line(windowd::VISIBLE_BACKEND_MARKER);
            }
            emit_line(windowd::PRESENT_VISIBLE_MARKER);
            if evidence.display.first_scanout_ready {
                emit_line(windowd::DISPLAY_FIRST_SCANOUT_MARKER);
            }
            if evidence.display.systemui_first_frame {
                emit_line(windowd::SYSTEMUI_FIRST_FRAME_VISIBLE_MARKER);
            }
            if !visible_present_marker_emitted {
                emit_line(windowd::SELFTEST_UI_VISIBLE_PRESENT_MARKER);
                visible_present_marker_emitted = true;
            }
            match evidence.runtime_mode {
                RuntimeMode::Proof => {
                    if let Some(proof) = evidence.proof {
                        proof_witness.observe(proof.visible_state);
                        proof_completed = emit_proof_mode_markers(proof_witness.observed_state());
                    }
                }
                RuntimeMode::InteractiveMinimal => {
                    proof_completed = false;
                    interactive_mode = Some(RuntimeMode::InteractiveMinimal);
                    if let Some(interactive) = evidence.interactive {
                        if interactive.scene_ready && interactive.full_window_visible {
                            emit_line(windowd::INTERACTIVE_SCENE_READY_MARKER);
                        }
                    }
                }
                RuntimeMode::InteractiveFull => {
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
                }
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

    if proof_completed {
        if auto_exit_after_proof {
            exit(0);
        }
    }

    // Stay alive (cooperative).
    let mut idle_ticks: u32 = 0;
    let mut last_interactive_poll_ns = nsec().unwrap_or(0);
    let mut last_interactive_poll_tick = 0u32;
    loop {
        idle_ticks = idle_ticks.wrapping_add(1);
        let should_poll_visible_state = !proof_completed
            || matches!(
                interactive_mode,
                Some(RuntimeMode::InteractiveMinimal | RuntimeMode::InteractiveFull)
            );
        if should_poll_visible_state
            && should_poll_interactive_visible_state(
                idle_ticks,
                &mut last_interactive_poll_tick,
                &mut last_interactive_poll_ns,
            )
        {
            if let Some(state) = display_bootstrap::interactive_live_tick() {
                if !visible_present_marker_emitted
                    && state.backend_visible
                    && state.display_scanout_ready
                    && state.systemui_first_frame_visible
                {
                    emit_line(windowd::SELFTEST_UI_VISIBLE_PRESENT_MARKER);
                    visible_present_marker_emitted = true;
                }
                if !proof_completed {
                    proof_witness.observe(state);
                }
                if !proof_completed && proof_witness.ready() {
                    proof_completed = emit_proof_mode_markers(proof_witness.observed_state());
                    if proof_completed {
                        if auto_exit_after_proof {
                            exit(0);
                        }
                    }
                }
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

fn emit_proof_mode_markers(visible_input: VisibleState) -> bool {
    match windowd::run_ui_v2a_smoke() {
        Ok(v2a) => {
            if v2a.present_scheduler_on {
                emit_line(windowd::PRESENT_SCHEDULER_ON_MARKER);
            } else {
                emit_line(V2A_SCHEDULER_OFF_MARKER);
            }
            if v2a.input_on {
                emit_line(windowd::INPUT_ON_MARKER);
                emit_bytes(b"windowd: focus -> ");
                emit_u64(v2a.focused_surface.raw());
                emit_bytes(b"\n");
                if v2a.launcher_click_ok {
                    emit_line(windowd::LAUNCHER_CLICK_OK_MARKER);
                } else {
                    emit_line(V2A_CLICK_OFF_MARKER);
                }
            } else {
                emit_line(V2A_INPUT_OFF_MARKER);
            }
            if v2a.present_scheduler_on {
                emit_line(windowd::SELFTEST_UI_V2_PRESENT_OK_MARKER);
            }
            if v2a.input_on && v2a.launcher_click_ok {
                emit_line(windowd::SELFTEST_UI_V2_INPUT_OK_MARKER);
            }
        }
        Err(_) => emit_line(V2A_SMOKE_ERR_MARKER),
    }
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
    if visible_input.wheel_up_visible || visible_input.wheel_down_visible {
        emit_line(windowd::WHEEL_VISIBLE_MARKER);
    }
    if visible_input.input_visible_on
        && visible_input.full_window_visible
        && visible_input.cursor_move_visible
        && visible_input.hover_visible
        && visible_input.focus_visible
        && visible_input.launcher_click_visible
        && visible_input.keyboard_visible
        && (visible_input.wheel_up_visible || visible_input.wheel_down_visible)
    {
        emit_line(windowd::SELFTEST_UI_VISIBLE_WHEEL_OK_MARKER);
        if proof_v2b_assets_ready(visible_input) {
            emit_line(windowd::SELFTEST_UI_V2B_ASSETS_OK_MARKER);
            return true;
        }
    }
    false
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
