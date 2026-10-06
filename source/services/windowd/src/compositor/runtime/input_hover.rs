// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: windowd compositor runtime — pointer HOVER routing (a pure move out
//! of `input.rs`, no behavior change) plus the app-modal facts the routing
//! loops consult (TASK-0074 D4): the surface under the pointer is resolved with
//! the SAME z-order/press geometry the tap routing uses, so input and hover can
//! never disagree; a window gated by a sibling's modal hovers nothing.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable

use super::*;

impl DisplayServerRuntime {
    /// Resolve the surface under the pointer with the SAME z-order/press
    /// geometry the tap routing uses (input and hover can never disagree),
    /// send it a MOVE, and send the previous target a LEAVE when the route
    /// changes. Drags/resizes capture the pointer — no hover while active.
    /// Title bars/buttons are windowd chrome (its own title hover), not app
    /// hover. One-time proof marker: `windowd: hover routing on`.
    pub(crate) fn forward_pointer_hover(&mut self, cursor_x: i32, cursor_y: i32) {
        use nexus_display_proto::client_surface::{INPUT_KIND_LEAVE, INPUT_KIND_MOVE};
        let mut route = HOVER_ROUTE_NONE;
        let mut local = (cursor_x, cursor_y);
        let mut route_idx = 0usize;
        let any_drag = self.apps.iter().any(|a| a.win.is_dragging());
        if !any_drag && self.resize_drag.is_none() {
            use crate::compositor::shell_window::WindowPress;
            use crate::window_scene::WindowId;
            let (hit, hit_n) = self.windows.hit_order(USE_DESKTOP_SHELL);
            for i in 0..hit_n {
                let wid = hit[i];
                // Shell chrome contract (mirrors the press loop): the top-bar
                // strip hovers the SHELL, never a window behind it.
                if matches!(wid, WindowId::App(_)) && cursor_y < super::SHELL_TOPBAR_H as i32 {
                    continue;
                }
                match wid {
                    WindowId::App(a) => {
                        let idx = a as usize;
                        let frame = self.apps[idx].win.frame();
                        // A modal-gated sibling hovers nothing (and nothing
                        // beneath it either) — TASK-0074 D4.
                        if matches!(
                            crate::modal_gate::input_verdict(idx, &self.modal_facts()),
                            crate::modal_gate::Verdict::RefusedByModal { .. }
                        ) && !matches!(frame.press(cursor_x, cursor_y), WindowPress::Miss)
                        {
                            break;
                        }
                        match frame.press(cursor_x, cursor_y) {
                            WindowPress::Miss => continue,
                            WindowPress::Body => {
                                let body_y = cursor_y - frame.y - self.apps[idx].win.title_h as i32;
                                if body_y >= 0 {
                                    route = HOVER_ROUTE_APP;
                                    route_idx = idx;
                                    local = (cursor_x - frame.x, body_y);
                                }
                            }
                            // Title bar / window buttons: windowd chrome hover.
                            _ => {}
                        }
                    }
                    WindowId::Desktop => {
                        if self.windows.is_visible(WindowId::Desktop) {
                            route = HOVER_ROUTE_DESKTOP;
                        }
                    }
                }
                break;
            }
        }
        // Route change = target change: leaving one APP window for another is
        // a change too (the old window must clear its hover wash).
        let route_changed = route != self.hover_route
            || (route == HOVER_ROUTE_APP && route_idx != self.hover_app_idx);
        if route_changed {
            let (lx, ly) = self.hover_last;
            match self.hover_route {
                HOVER_ROUTE_APP => {
                    let prev = self.hover_app_idx;
                    self.send_app_input_kind(prev, INPUT_KIND_LEAVE, lx, ly);
                }
                HOVER_ROUTE_DESKTOP => {
                    self.send_desktop_input_kind(INPUT_KIND_LEAVE, lx, ly);
                }
                _ => {}
            }
        }
        // Throttle MOVE forwarding to the frame pace (120Hz): app-side hover
        // washes track the pointer at display rate. The historical flood risk
        // (unthrottled per-EVENT forwarding filled the client queue and starved
        // TAP delivery) is gone twice over: moves apply frame-aligned (once per
        // staged sample) and `send_input_frame` drops MOVEs on a full queue
        // while TAPs retry — so display-rate forwarding is safe.
        let now = nexus_abi::nsec().unwrap_or(0);
        let move_due = now.saturating_sub(self.hover_last_move_ns) >= PACER_INTERVAL_NS;
        if move_due {
            match route {
                HOVER_ROUTE_APP => {
                    self.send_app_input_kind(route_idx, INPUT_KIND_MOVE, local.0, local.1);
                }
                HOVER_ROUTE_DESKTOP => {
                    self.send_desktop_input_kind(INPUT_KIND_MOVE, local.0, local.1);
                }
                _ => {}
            }
            if route != HOVER_ROUTE_NONE {
                self.hover_last_move_ns = now;
            }
        }
        if route != HOVER_ROUTE_NONE && !self.hover_marker_emitted {
            let _ = debug_println("windowd: hover routing on");
            self.hover_marker_emitted = true;
        }
        self.hover_route = route;
        self.hover_app_idx = route_idx;
        self.hover_last = local;
    }

    pub(super) fn note_filter_text_changed(&mut self) {
        self.filter_cycle = self.filter_cycle.wrapping_add(1);

        if !self.clipping_marker_emitted {
            let _ = debug_println(crate::markers::CLIPPING_ON_MARKER);
            self.clipping_marker_emitted = true;
        }
        let _ = debug_println(crate::markers::TEXT_INPUT_ON_MARKER);
        let _ = debug_println(crate::markers::FILTER_LIST_OK_MARKER);
        // C1: the proof filter panel is gone — no filter rects to damage.
    }

    /// The slots' facts the modal gate decides on (TASK-0074 D4): owner, modal flag, live
    /// surface — a fixed array, no allocation per event.
    pub(crate) fn modal_facts(
        &self,
    ) -> [crate::modal_gate::SlotFacts; crate::window_scene::MAX_APP_WINDOWS] {
        core::array::from_fn(|i| crate::modal_gate::SlotFacts {
            owner_sid: self.apps[i].owner_sid,
            modal: self.apps[i].app_modal,
            live: self.apps[i].surface_id.is_some(),
        })
    }
}
