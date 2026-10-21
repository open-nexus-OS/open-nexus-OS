// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: windowd compositor runtime — WINDOW TRANSITIONS on the window transform (Track
//! C2+C3): open = fade+scale-in, close = fade-out then close, minimize = fly-to-taskbar then
//! minimize. windowd's own `AnimationDriver` interpolates the springs on the frame clock (the
//! display's acks); every tick folds into the slot's `WinTransform` and OWES a frame: the next
//! present carries every window's transform (`Command::LayerTransform`) beside the unchanged
//! layers and names no damage — the planner redraws where the window was and is (TASK-0338: the
//! present is the only frame, inside the budget of two in flight). NO app re-render, NO band
//! re-blit, NO CPU recomposite per animation frame.
//! OWNERS: @ui
//! STATUS: Functional (Track C3)
//! API_STABILITY: Unstable

use super::*;

/// The window-transform layer-id namespace in windowd's own `AnimationDriver`
/// (distinct from the chrome proof layers' small consts).
const WT_LAYER_BASE: u64 = 0x5731_0000;

/// Springs: entries are swift and lightly damped; exits are quicker + smooth
/// (the motion vocabulary's enter/exit asymmetry).
const ENTER_SPRING: animation::SpringConfig =
    animation::SpringConfig { stiffness: 260.0, damping: 24.0, mass: 1.0, initial_velocity: 0.0 };
const EXIT_SPRING: animation::SpringConfig =
    animation::SpringConfig { stiffness: 380.0, damping: 34.0, mass: 1.0, initial_velocity: 0.0 };

/// Open/enter scale start (the decided `fadeScale` look).
const OPEN_SCALE_FROM: f32 = 0.92;
/// Close/exit scale target.
const CLOSE_SCALE_TO: f32 = 0.92;
/// Minimize fly target scale.
const MIN_SCALE_TO: f32 = 0.15;

impl DisplayServerRuntime {
    #[inline]
    fn wt_layer(idx: usize) -> animation::LayerId {
        animation::LayerId(WT_LAYER_BASE + idx as u64)
    }

    /// Decode a driver layer id back to a window slot (None = not a window
    /// transform layer).
    pub(super) fn wt_slot(layer: animation::LayerId) -> Option<usize> {
        let v = layer.0.checked_sub(WT_LAYER_BASE)?;
        (v < crate::window_scene::MAX_APP_WINDOWS as u64).then_some(v as usize)
    }

    /// OPEN transition: the window fades + scales IN from 92% (the decided
    /// enter look). Called for a FRESH window right after its first mount —
    /// the first present carries the initial (transparent) transform with the
    /// window's layers, and each tick's present carries it on to identity.
    pub(super) fn start_open_transition(&mut self, idx: usize) {
        let t = &mut self.apps[idx].transform;
        *t = WinTransform { dx: 0.0, dy: 0.0, opacity: 0.0, scale: OPEN_SCALE_FROM, active: true };
        let layer = Self::wt_layer(idx);
        let now = nexus_abi::nsec().unwrap_or(0);
        self.animation_driver.reset_clock(now);
        self.animation_driver.spring_to(layer, AnimProp::Opacity, 0.0, 1.0, ENTER_SPRING);
        self.animation_driver.spring_to(
            layer,
            AnimProp::ScaleX,
            OPEN_SCALE_FROM,
            1.0,
            ENTER_SPRING,
        );
        // The next present carries the INITIAL state with the window's layers: it composites
        // from transparent instead of flashing full-on for one frame.
        self.owe_frame();
        let _ = debug_println("windowd: transition open");
    }

    /// CLOSE transition: fade + scale out, then the deferred
    /// `close_app_window` runs on convergence.
    pub(super) fn start_close_transition(&mut self, idx: usize) {
        if self.apps[idx].pending_wm.is_some() {
            return; // already leaving
        }
        let cur = self.apps[idx].transform;
        self.apps[idx].transform.active = true;
        self.apps[idx].pending_wm = Some(PendingWm::Close);
        // The content leaves at once (TASK-0336): the exit spring moves the window's closing
        // surface — one opaque solid layer — never the app's layers with their glass.
        let _ = self.show_solid(idx);
        let layer = Self::wt_layer(idx);
        let now = nexus_abi::nsec().unwrap_or(0);
        self.animation_driver.reset_clock(now);
        self.animation_driver.spring_to(layer, AnimProp::Opacity, cur.opacity, 0.0, EXIT_SPRING);
        self.animation_driver.spring_to(
            layer,
            AnimProp::ScaleX,
            cur.scale,
            CLOSE_SCALE_TO,
            EXIT_SPRING,
        );
        let _ = debug_println("windowd: transition close");
    }

    /// MINIMIZE transition: fly toward the shell taskbar band
    /// (translate + shrink + fade), then the deferred `minimize_window` runs
    /// on convergence.
    pub(super) fn start_minimize_transition(&mut self, idx: usize) {
        if self.apps[idx].pending_wm.is_some() {
            return;
        }
        let cur = self.apps[idx].transform;
        self.apps[idx].transform.active = true;
        self.apps[idx].pending_wm = Some(PendingWm::Minimize);
        // Fly target as a delta from the window center (the layer scale is
        // CENTER-anchored in gpud — center-to-center math).
        // RFC-0086: fly toward the SHELL taskbar band's centre — windowd
        // owns the bar HEIGHT as work-area policy but never the shell's tile
        // positions (a per-tile target would couple the compositor to shell
        // layout; parked follow-up in the RFC).
        let (cell_cx, cell_cy) = self.taskbar_anchor();
        let win = &self.apps[idx].win;
        let target_x = cell_cx - (win.x as f32 + win.w as f32 / 2.0);
        let target_y = cell_cy - (win.y as f32 + win.h as f32 / 2.0);
        let layer = Self::wt_layer(idx);
        let now = nexus_abi::nsec().unwrap_or(0);
        self.animation_driver.reset_clock(now);
        self.animation_driver.spring_to(layer, AnimProp::TranslateX, cur.dx, target_x, EXIT_SPRING);
        self.animation_driver.spring_to(layer, AnimProp::TranslateY, cur.dy, target_y, EXIT_SPRING);
        self.animation_driver.spring_to(layer, AnimProp::Opacity, cur.opacity, 0.15, EXIT_SPRING);
        self.animation_driver.spring_to(
            layer,
            AnimProp::ScaleX,
            cur.scale,
            MIN_SCALE_TO,
            EXIT_SPRING,
        );
        let _ = debug_println("windowd: transition minimize");
    }

    /// RESTORE transition: the window flies IN from the taskbar anchor (the
    /// reverse of minimize). Unlike close/minimize the WM state change runs UP
    /// FRONT (`restore_window` re-mounts + raises + focuses), then the springs
    /// carry the transform from that origin to identity — convergence
    /// falls into `finish_window_transitions`' `None` arm (settle at
    /// identity), no `PendingWm` needed.
    pub(super) fn start_restore_transition(
        &mut self,
        id: crate::window_scene::WindowId,
        from_cx: f32,
        from_cy: f32,
    ) {
        let crate::window_scene::WindowId::App(i) = id else {
            return;
        };
        let idx = i as usize;
        if !self.windows.is_minimized(id) || self.apps[idx].pending_wm.is_some() {
            return;
        }
        self.restore_window(id);
        let win = &self.apps[idx].win;
        let dx = from_cx - (win.x as f32 + win.w as f32 / 2.0);
        let dy = from_cy - (win.y as f32 + win.h as f32 / 2.0);
        self.apps[idx].transform =
            WinTransform { dx, dy, opacity: 0.15, scale: MIN_SCALE_TO, active: true };
        let layer = Self::wt_layer(idx);
        let now = nexus_abi::nsec().unwrap_or(0);
        self.animation_driver.reset_clock(now);
        self.animation_driver.spring_to(layer, AnimProp::TranslateX, dx, 0.0, ENTER_SPRING);
        self.animation_driver.spring_to(layer, AnimProp::TranslateY, dy, 0.0, ENTER_SPRING);
        self.animation_driver.spring_to(layer, AnimProp::Opacity, 0.15, 1.0, ENTER_SPRING);
        self.animation_driver.spring_to(layer, AnimProp::ScaleX, MIN_SCALE_TO, 1.0, ENTER_SPRING);
        // The restore's queued present carries the taskbar-origin state with the re-mounted
        // frame (TASK-0336: atomic — no full-size flash, no frame shown at the old place).
        let _ = debug_println("windowd: transition restore");
    }

    /// FULLSCREEN transition (enter AND leave): the geometry flips instantly
    /// (`toggle_fullscreen` — band re-create is async, the resize path shows
    /// the old content clamped inside the growing glass frame), then the
    /// transform seeds the OLD frame's apparent rect relative to the NEW frame
    /// and springs to identity — the window visibly grows/shrinks between the
    /// two frames while the client re-renders underneath.
    pub(super) fn start_fullscreen_transition(&mut self, id: crate::window_scene::WindowId) {
        let crate::window_scene::WindowId::App(i) = id else {
            return;
        };
        let idx = i as usize;
        if self.apps[idx].pending_wm.is_some() {
            return;
        }
        let w0 = &self.apps[idx].win;
        let (old_cx, old_cy, old_w) =
            (w0.x as f32 + w0.w as f32 / 2.0, w0.y as f32 + w0.h as f32 / 2.0, w0.w as f32);
        self.toggle_fullscreen(id);
        let w1 = &self.apps[idx].win;
        let (new_cx, new_cy, new_w) =
            (w1.x as f32 + w1.w as f32 / 2.0, w1.y as f32 + w1.h as f32 / 2.0, w1.w as f32);
        if new_w <= 0.0 || (new_w - old_w).abs() < 1.0 {
            // Geometry did not change — nothing to animate. But never leave a
            // previous transition's non-identity transform standing on the
            // flipped frame: a stale scale/offset here rendered the window
            // displaced until the next transition (which `pending_wm` might
            // never allow).
            if self.apps[idx].transform.active {
                self.apps[idx].transform = WinTransform::IDENTITY;
                self.owe_frame();
            }
            return;
        }
        let scale_from = (old_w / new_w).clamp(0.05, 4.0);
        let dx = old_cx - new_cx;
        let dy = old_cy - new_cy;
        self.apps[idx].transform =
            WinTransform { dx, dy, opacity: 1.0, scale: scale_from, active: true };
        let layer = Self::wt_layer(idx);
        let now = nexus_abi::nsec().unwrap_or(0);
        self.animation_driver.reset_clock(now);
        self.animation_driver.spring_to(layer, AnimProp::TranslateX, dx, 0.0, ENTER_SPRING);
        self.animation_driver.spring_to(layer, AnimProp::TranslateY, dy, 0.0, ENTER_SPRING);
        self.animation_driver.spring_to(layer, AnimProp::ScaleX, scale_from, 1.0, ENTER_SPRING);
        // The toggle's queued full present carries the seed with the flipped frame (TASK-0336:
        // atomic — the old frame never shows the seed, the new one never shows identity).
        let _ = debug_println("windowd: transition fullscreen");
    }

    /// Fold one driver update into the slot transform. Returns true when it
    /// targeted a window-transform layer.
    pub(super) fn apply_window_transform_update(&mut self, u: &SceneUpdate) -> bool {
        let Some(idx) = Self::wt_slot(u.layer_id) else { return false };
        if idx >= self.apps.len() {
            return true;
        }
        {
            let t = &mut self.apps[idx].transform;
            match u.property {
                AnimProp::Opacity => t.opacity = u.value.clamp(0.0, 1.0),
                AnimProp::TranslateX => t.dx = u.value,
                AnimProp::TranslateY => t.dy = u.value,
                AnimProp::ScaleX | AnimProp::ScaleY => t.scale = u.value.max(0.01),
                _ => {}
            }
            t.active = true;
        }
        self.owe_frame();
        true
    }

    /// The window's transform as gpud draws it now (TASK-0336): the transition's — moved,
    /// faded, scaled about the centre — plus a drag's offset; in the wire's units (px, px,
    /// `0..=255`, percent). Every present carries it (`Command::LayerTransform`), the only way
    /// it reaches the display (TASK-0338).
    pub(super) fn wire_transform(&self, idx: usize) -> (i32, i32, u32, u32) {
        let t = self.apps[idx].transform;
        let (ddx, ddy) = self.apps[idx].drag_offset.unwrap_or((0, 0));
        (
            t.dx as i32 + ddx,
            t.dy as i32 + ddy,
            (t.opacity * 255.0).clamp(0.0, 255.0) as u32,
            (t.scale * 100.0).clamp(1.0, 400.0) as u32,
        )
    }

    /// Every window slot's transform for the present being built: a mounted (or starting)
    /// window's as it draws now, the identity for a free slot — so a slot's next tenant never
    /// inherits the last tenant's fade (TASK-0338: no op retires it any more).
    pub(super) fn present_transforms(
        &self,
    ) -> [(i32, i32, u32, u32); crate::window_scene::MAX_APP_WINDOWS] {
        const IDENTITY: (i32, i32, u32, u32) = (0, 0, 255, 100);
        let mut transforms = [IDENTITY; crate::window_scene::MAX_APP_WINDOWS];
        for (idx, t) in transforms.iter_mut().enumerate().take(self.apps.len()) {
            if self.apps[idx].surface_id.is_some() || self.apps[idx].starting {
                *t = self.wire_transform(idx);
            }
        }
        transforms
    }

    /// After a driver tick: finish transitions whose springs all converged —
    /// run the deferred WM action (close/minimize) or settle to identity.
    pub(super) fn finish_window_transitions(&mut self) {
        for idx in 0..self.apps.len() {
            if !self.apps[idx].transform.active {
                continue;
            }
            let layer = Self::wt_layer(idx);
            let still = self.animation_driver.is_active(layer, AnimProp::Opacity)
                || self.animation_driver.is_active(layer, AnimProp::ScaleX)
                || self.animation_driver.is_active(layer, AnimProp::TranslateX)
                || self.animation_driver.is_active(layer, AnimProp::TranslateY);
            if still {
                continue;
            }
            let pending = self.apps[idx].pending_wm.take();
            self.apps[idx].transform = WinTransform::IDENTITY;
            match pending {
                Some(PendingWm::Close) => {
                    self.close_app_window(idx);
                }
                Some(PendingWm::Minimize) => {
                    self.minimize_window(crate::window_scene::WindowId::App(idx as u8));
                    // The parked window re-enters at identity on restore.
                    self.owe_frame();
                }
                None => {
                    // Open settled: one final frame at the exact identity.
                    self.owe_frame();
                }
            }
        }
    }
}
