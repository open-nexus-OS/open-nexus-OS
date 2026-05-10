// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Narrow state-merge helpers for the service-owned display observer path.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Covered through `fbdevd` host tests.
//! ADR: docs/adr/0028-windowd-surface-present-and-visible-bootstrap-architecture.md

use input_live_protocol::VisibleState;

#[cfg(all(feature = "os-lite", nexus_env = "os", target_os = "none"))]
pub const ROUTE_NAME: &str = "fbdevd";

pub fn merge_visible_state(
    previous: VisibleState,
    upstream: VisibleState,
    backend_visible: bool,
    display_scanout_ready: bool,
    systemui_first_frame_visible: bool,
) -> VisibleState {
    VisibleState {
        backend_visible,
        display_scanout_ready,
        systemui_first_frame_visible,
        scene_ready: previous.scene_ready || upstream.scene_ready,
        full_window_visible: previous.full_window_visible || upstream.full_window_visible,
        click_target_visible: previous.click_target_visible || upstream.click_target_visible,
        keyboard_target_visible: previous.keyboard_target_visible
            || upstream.keyboard_target_visible,
        ..upstream
    }
}

pub fn merge_observer_visible_state(
    previous: VisibleState,
    upstream: VisibleState,
    backend_visible: bool,
    display_scanout_ready: bool,
    systemui_first_frame_visible: bool,
) -> VisibleState {
    VisibleState {
        backend_visible,
        display_scanout_ready,
        systemui_first_frame_visible,
        virtio_raw_seen: previous.virtio_raw_seen || upstream.virtio_raw_seen,
        hid_normalized_seen: previous.hid_normalized_seen || upstream.hid_normalized_seen,
        scene_ready: previous.scene_ready || upstream.scene_ready,
        full_window_visible: previous.full_window_visible || upstream.full_window_visible,
        click_target_visible: previous.click_target_visible || upstream.click_target_visible,
        keyboard_target_visible: previous.keyboard_target_visible
            || upstream.keyboard_target_visible,
        input_visible_on: previous.input_visible_on || upstream.input_visible_on,
        cursor_move_visible: previous.cursor_move_visible || upstream.cursor_move_visible,
        hover_visible: previous.hover_visible || upstream.hover_visible,
        focus_visible: previous.focus_visible || upstream.focus_visible,
        launcher_click_visible: previous.launcher_click_visible || upstream.launcher_click_visible,
        keyboard_visible: previous.keyboard_visible || upstream.keyboard_visible,
        wheel_up_visible: previous.wheel_up_visible || upstream.wheel_up_visible,
        wheel_down_visible: previous.wheel_down_visible || upstream.wheel_down_visible,
        pointer_route_live: previous.pointer_route_live || upstream.pointer_route_live,
        keyboard_route_live: previous.keyboard_route_live || upstream.keyboard_route_live,
        cursor_x: upstream.cursor_x,
        cursor_y: upstream.cursor_y,
    }
}

pub fn display_ready_for_observer(state: VisibleState) -> bool {
    state.backend_visible && state.display_scanout_ready && state.systemui_first_frame_visible
}
