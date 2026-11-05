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
use nexus_display_proto::surface_motion::MotionBatch;

/// The surface that took the primary press (RFC-0095, the drag gesture): while the button is
/// held it receives `INPUT_KIND_DRAG` instead of hover moves, and `INPUT_KIND_RELEASE` at the end.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PressTarget {
    Desktop,
    App(usize),
}

/// A held press: its surface, and the newest drag position that surface's full queue refused —
/// sent again on the next loop pass, so the last position always arrives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PressRoute {
    target: PressTarget,
    owed: Option<(i32, i32)>,
}

impl PressRoute {
    pub(crate) const fn desktop() -> Self {
        Self { target: PressTarget::Desktop, owed: None }
    }

    pub(crate) const fn app(idx: usize) -> Self {
        Self { target: PressTarget::App(idx), owed: None }
    }
}

static DESKTOP_TAP_SAID: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);
static SURFACE_TAP_SAID: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);

/// Whether this is the first routed tap (to the desktop, or to an app window) of the boot. The
/// "input routed" line is said once: a line per tap would log keystroke timing — the on-screen
/// keyboard is an app window (privacy rule).
pub(super) fn first_tap_routed(desktop: bool) -> bool {
    let said = if desktop { &DESKTOP_TAP_SAID } else { &SURFACE_TAP_SAID };
    !said.swap(true, core::sync::atomic::Ordering::Relaxed)
}

impl DisplayServerRuntime {
    /// Whether the pointer at `(x, y)` belongs to the SHELL above every app window
    /// (`crate::shell_band`): the top-bar strip, a panel-level glass rect of the desktop
    /// surface (composited above the windows, scene pass 2b), a layer it declared in front
    /// (its toast — pass 2c; one over a backdrop covers everything) or anywhere while the shell
    /// holds a modal. The press, hover and wheel loops skip app windows there, so input
    /// lands where the pixels are.
    pub(super) fn shell_owns_point(&self, x: i32, y: i32) -> bool {
        use nexus_display_proto::client_surface as wire;
        use nexus_display_proto::surface_layers::MAX_MOTION_LAYERS;
        let mut panels = [(0i32, 0i32, 0u32, 0u32); wire::MAX_SURFACE_LAYERS + MAX_MOTION_LAYERS];
        let mut n = 0;
        for l in self.desktop_layers.tree.regions() {
            if l.material == wire::MATERIAL_GLASS && l.glass_level == wire::GLASS_PANEL {
                panels[n] = (i32::from(l.x), i32::from(l.y), u32::from(l.w), u32::from(l.h));
                n += 1;
            }
        }
        let mut scrim = false;
        for m in self.desktop_layers.tree.motions().iter().filter(|m| m.front) {
            panels[n] = (i32::from(m.x), i32::from(m.y), u32::from(m.w), u32::from(m.h));
            n += 1;
            scrim |= m.backdrop().is_some();
        }
        crate::shell_band::shell_owns_point(
            x,
            y,
            super::SHELL_TOPBAR_H,
            &panels[..n],
            self.desktop_modal || scrim,
        )
    }

    /// Resolve the surface under the pointer with the SAME z-order/press
    /// geometry the tap routing uses (input and hover can never disagree),
    /// send it a MOVE, and send the previous target a LEAVE when the route
    /// changes. Drags/resizes capture the pointer — no hover while active.
    /// Title bars/buttons are windowd chrome (its own title hover), not app
    /// hover. One-time proof marker: `windowd: hover routing on`.
    pub(crate) fn forward_pointer_hover(&mut self, cursor_x: i32, cursor_y: i32) {
        use nexus_display_proto::client_surface::{INPUT_KIND_LEAVE, INPUT_KIND_MOVE};
        let motion = core::mem::take(&mut self.motion.applied);
        // A held press drags on the surface that took it — no hover routing meanwhile.
        if self.forward_drag(cursor_x, cursor_y, &motion) {
            return;
        }
        let mut route = HOVER_ROUTE_NONE;
        let mut local = (cursor_x, cursor_y);
        let mut route_idx = 0usize;
        let any_drag = self.apps.iter().any(|a| a.win.is_dragging());
        if !any_drag && self.resize_drag.is_none() {
            use crate::compositor::shell_window::WindowPress;
            use crate::window_scene::WindowId;
            let (hit, hit_n) = self.windows.hit_order(self.capture.frozen());
            for i in 0..hit_n {
                let wid = hit[i];
                // Shell chrome contract (mirrors the press loop): the top-bar
                // strip, the shell's panels and a shell modal hover the SHELL,
                // never a window behind them.
                if matches!(wid, WindowId::App(_)) && self.shell_owns_point(cursor_x, cursor_y) {
                    continue;
                }
                match wid {
                    WindowId::App(a) => {
                        let idx = a as usize;
                        let frame = self.apps[idx].win.frame();
                        // A modal-gated sibling hovers nothing (and nothing
                        // beneath it either) — TASK-0074 D4.
                        if matches!(
                            self.modal_verdict(idx),
                            crate::modal_gate::Verdict::RefusedByModal { .. }
                        ) && !matches!(frame.press(cursor_x, cursor_y), WindowPress::Miss)
                        {
                            break;
                        }
                        match frame.press(cursor_x, cursor_y) {
                            WindowPress::Miss => continue,
                            WindowPress::Body => {
                                let body_y = cursor_y - frame.y;
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
            // The held-back samples were the old surface's (RFC-0113 §2).
            self.motion.hover.clear();
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
        // staged sample) and a busy client's outbox keeps ONE pending move while
        // taps queue in order (ADR-0075) — so display-rate forwarding is safe.
        let now = nexus_abi::nsec().unwrap_or(0);
        let move_due = now.saturating_sub(self.hover_last_move_ns) >= PACER_INTERVAL_NS;
        // A MOVE carries every sample since the last one to this surface (RFC-0113 §2).
        self.motion.hover.append(&motion);
        if move_due {
            let (dx, dy) = (local.0 - cursor_x, local.1 - cursor_y);
            self.motion.out = core::mem::take(&mut self.motion.hover).shifted(dx, dy);
            match route {
                HOVER_ROUTE_APP => {
                    self.send_app_input_kind(route_idx, INPUT_KIND_MOVE, local.0, local.1);
                }
                HOVER_ROUTE_DESKTOP => {
                    self.send_desktop_input_kind(INPUT_KIND_MOVE, local.0, local.1);
                }
                _ => {}
            }
            self.motion.out.clear(); // staged for this MOVE only, sent or not
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

    /// The pointer in `target`'s surface coordinates (an app window's body starts below its
    /// resolved chrome height).
    fn press_local(&self, target: PressTarget, x: i32, y: i32) -> (i32, i32) {
        match target {
            PressTarget::Desktop => (x, y),
            PressTarget::App(idx) => {
                let frame = self.apps[idx].win.frame();
                (x - frame.x, y - frame.y)
            }
        }
    }

    /// One frame of `kind` to the press's surface; `false` when it did not go out.
    fn send_press_kind(&mut self, target: PressTarget, kind: u8, x: i32, y: i32) -> bool {
        let (lx, ly) = self.press_local(target, x, y);
        let sent = match target {
            PressTarget::Desktop => self.send_desktop_input_kind(kind, lx, ly),
            PressTarget::App(idx) => self.send_app_input_kind(idx, kind, lx, ly),
        };
        self.motion.out.clear(); // staged for this frame only, sent or not
        sent
    }

    /// While the primary button holds a press, the pointer is a DRAG to the surface that took
    /// it — wherever the pointer is. Every applied sample goes out (one per loop pass, already
    /// frame-aligned) with its pointer samples; the surface computes only the newest it holds.
    /// A sample its full queue refused is owed (`flush_drag`), its samples kept. `true` when it
    /// was one.
    fn forward_drag(&mut self, x: i32, y: i32, motion: &MotionBatch) -> bool {
        use nexus_display_proto::client_surface::INPUT_KIND_DRAG;
        let Some(mut press) = self.press_route.filter(|_| self.state.launcher_click_visible) else {
            return false;
        };
        self.motion.owed.append(motion);
        self.stage_drag_motion(press.target, x, y);
        let sent = self.send_press_kind(press.target, INPUT_KIND_DRAG, x, y);
        press.owed = (!sent).then_some((x, y));
        if sent {
            self.motion.owed.clear();
        }
        self.press_route = Some(press);
        true
    }

    /// The owed drag samples as the press's surface sees them, for the DRAG about to go out.
    fn stage_drag_motion(&mut self, target: PressTarget, x: i32, y: i32) {
        let (lx, ly) = self.press_local(target, x, y);
        self.motion.out = self.motion.owed.shifted(lx - x, ly - y);
    }

    /// The loop pass's retry of a drag position a full queue refused — reactive: the next pass
    /// comes with the surface's next present (or any other event), never on a clock.
    pub(crate) fn flush_drag(&mut self) {
        use nexus_display_proto::client_surface::INPUT_KIND_DRAG;
        let Some(press) = self.press_route else { return };
        let Some((x, y)) = press.owed else { return };
        self.stage_drag_motion(press.target, x, y);
        if self.send_press_kind(press.target, INPUT_KIND_DRAG, x, y) {
            self.press_route = Some(PressRoute { owed: None, ..press });
            self.motion.owed.clear();
        }
    }

    /// The primary button went up: the surface that took the press hears the RELEASE at the
    /// release point (it supersedes an owed drag position; parked on like a tap).
    pub(crate) fn release_press_route(&mut self, x: i32, y: i32) {
        use nexus_display_proto::client_surface::INPUT_KIND_RELEASE;
        if let Some(press) = self.press_route.take() {
            self.motion.owed.clear(); // the release supersedes an owed drag, samples and all
            self.send_press_kind(press.target, INPUT_KIND_RELEASE, x, y);
        }
    }

    /// Whether input to slot `idx` passes the modal gate (TASK-0074 D4): the gate reads each
    /// slot's facts — owner, modal flag, live surface — in place, no table per event (the slots
    /// grow, RFC-0114 §7).
    pub(crate) fn modal_verdict(&self, idx: usize) -> crate::modal_gate::Verdict {
        crate::modal_gate::input_verdict_by(idx, self.apps.len(), |i| {
            crate::modal_gate::SlotFacts {
                owner_sid: self.apps[i].owner_sid,
                modal: self.apps[i].app_modal,
                live: self.apps[i].surface_id.is_some(),
            }
        })
    }
}
