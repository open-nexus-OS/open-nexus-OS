// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Pure observer-side predicates for the service-owned display path.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Internal
//! TEST_COVERAGE: Host tests in `source/apps/selftest-client/tests/`.
//! ADR: docs/adr/0027-selftest-client-two-axis-architecture.md

use input_live_protocol::VisibleState;

pub(crate) fn display_bootstrap_ready(state: VisibleState) -> bool {
    state.backend_visible && state.display_scanout_ready && state.systemui_first_frame_visible
}

pub(crate) fn proof_visible_input_ready(state: VisibleState) -> bool {
    display_bootstrap_ready(state)
        && state.virtio_raw_seen
        && state.hid_normalized_seen
        && state.scene_ready
        && state.full_window_visible
        && state.click_target_visible
        && state.keyboard_target_visible
        && state.input_visible_on
        && state.cursor_move_visible
        && state.hover_visible
        && state.focus_visible
        && state.launcher_click_visible
        && state.keyboard_visible
        && state.pointer_route_live
        && state.keyboard_route_live
}

pub(crate) fn interactive_scene_ready(state: VisibleState) -> bool {
    display_bootstrap_ready(state)
        && state.scene_ready
        && state.full_window_visible
        && state.click_target_visible
        && state.keyboard_target_visible
}

#[cfg(all(nexus_env = "os", target_os = "none"))]
fn emit_debug(label: &str) {
    let _ = nexus_abi::debug_println(label);
}

#[cfg(not(all(nexus_env = "os", target_os = "none")))]
fn emit_debug(_label: &str) {}

pub(crate) fn emit_missing_visible_input_bits(state: VisibleState) {
    if !state.backend_visible {
        emit_debug("bootstrap: missing backend-visible");
    }
    if !state.display_scanout_ready {
        emit_debug("bootstrap: missing display-scanout-ready");
    }
    if !state.systemui_first_frame_visible {
        emit_debug("bootstrap: missing systemui-first-frame");
    }
    if !state.virtio_raw_seen {
        emit_debug("bootstrap: missing virtio-raw");
    }
    if !state.hid_normalized_seen {
        emit_debug("bootstrap: missing hid-normalized");
    }
    if !state.scene_ready {
        emit_debug("bootstrap: missing scene-ready");
    }
    if !state.full_window_visible {
        emit_debug("bootstrap: missing full-window");
    }
    if !state.click_target_visible {
        emit_debug("bootstrap: missing click-target");
    }
    if !state.keyboard_target_visible {
        emit_debug("bootstrap: missing keyboard-target");
    }
    if !state.input_visible_on {
        emit_debug("bootstrap: missing input-visible-on");
    }
    if !state.cursor_move_visible {
        emit_debug("bootstrap: missing cursor-move");
    }
    if !state.hover_visible {
        emit_debug("bootstrap: missing hover-visible");
    }
    if !state.focus_visible {
        emit_debug("bootstrap: missing focus-visible");
    }
    if !state.launcher_click_visible {
        emit_debug("bootstrap: missing launcher-click");
    }
    if !state.keyboard_visible {
        emit_debug("bootstrap: missing keyboard-visible");
    }
    if !state.pointer_route_live {
        emit_debug("bootstrap: missing pointer-route-live");
    }
    if !state.keyboard_route_live {
        emit_debug("bootstrap: missing keyboard-route-live");
    }
}
