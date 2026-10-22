// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: windowd compositor runtime — the wheel's ROUTING: the surface under the pointer (the
//! topmost window wins; the shell's bar, panels and modal above every window) gets the notch as
//! `OP_SURFACE_INPUT` kind=WHEEL, like every input. What a notch DOES is the app's (ADR-0076):
//! the app runs the one scroll physics and answers with motion — the scroll layer's new offset —
//! which windowd composes (RFC-0109). windowd runs no physics.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable

use super::*;

impl DisplayServerRuntime {
    /// Route a wheel delta to the surface under the pointer (the TOPMOST
    /// window wins — the retired `resolve_wheel_target`'s contract, now on
    /// the one z-stack SSOT). Sent as `OP_SURFACE_INPUT` kind=WHEEL with the
    /// signed delta riding the `y` field (`wheel_delta_to_wire`); the app takes
    /// the pointer's position from the move that preceded it. Intent
    /// (ADR-0075): a wheel step is never merged away — it is distance. Alloc-free.
    pub(crate) fn forward_wheel(&mut self, cursor_x: i32, cursor_y: i32, delta_y: i32) {
        use crate::compositor::shell_window::WindowPress;
        use crate::window_scene::WindowId;
        use nexus_display_proto::client_surface as wire;
        let wire_delta = wire::wheel_delta_to_wire(delta_y);
        if self.wheel_route_count < 40 {
            self.wheel_route_count += 1;
            let _ = debug_println(&alloc::format!(
                "windowd: wheel fwd n={} d={delta_y}",
                self.wheel_route_count
            ));
        }
        let (hit, hit_n) = self.windows.hit_order(self.capture.frozen());
        for i in 0..hit_n {
            let wid = hit[i];
            // The shell's bar, panels and modal take the wheel above any window (`shell_band`).
            if matches!(wid, WindowId::App(_)) && self.shell_owns_point(cursor_x, cursor_y) {
                continue;
            }
            match wid {
                WindowId::App(a) => {
                    let idx = a as usize;
                    let frame = self.apps[idx].win.frame();
                    // TASK-0074 D4: a modal-gated sibling takes no wheel.
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
                            let local_x = (cursor_x - frame.x).max(0) as u16;
                            self.send_app_wheel(idx, local_x, wire_delta);
                        }
                        // Title bar / buttons: chrome, not app scroll.
                        _ => {}
                    }
                }
                WindowId::Desktop => {
                    if self.windows.is_visible(WindowId::Desktop) {
                        self.send_desktop_wheel(cursor_x.max(0) as u16, wire_delta);
                    }
                }
            }
            break;
        }
    }
}
