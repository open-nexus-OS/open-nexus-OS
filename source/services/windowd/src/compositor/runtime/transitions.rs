// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: windowd compositor runtime — WINDOW TRANSITIONS on the unified
//! layer-transform primitive (Track C2+C3, the scroll generalization): open =
//! fade+scale-in, close = fade-out then close, minimize = fly-to-dock then
//! minimize. windowd's own `AnimationDriver` interpolates the springs on the
//! 120Hz pacer (all three tick paths — the fling lesson); every tick folds
//! into the slot's `WinTransform` and emits ONE fire-and-forget
//! `OP_SET_LAYER_TRANSFORM` — gpud records it and re-composites the retained
//! layer (record + coalesced flush, the scroll contract). NO app re-render,
//! NO band re-blit, NO CPU recomposite per animation frame — the 120Hz+ path.
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
    /// the first full present bakes the initial (transparent) transform, and
    /// the pacer-driven overrides carry it to identity on the GPU.
    pub(super) fn start_open_transition(&mut self, idx: usize) {
        let t = &mut self.apps[idx].transform;
        *t = WinTransform { dx: 0.0, dy: 0.0, opacity: 0.0, scale: OPEN_SCALE_FROM, active: true };
        let layer = Self::wt_layer(idx);
        let now = nexus_abi::nsec().unwrap_or(0);
        self.animation_driver.reset_clock(now);
        self.animation_driver.spring_to(layer, AnimProp::Opacity, 0.0, 1.0, ENTER_SPRING);
        self.animation_driver.spring_to(layer, AnimProp::ScaleX, OPEN_SCALE_FROM, 1.0, ENTER_SPRING);
        // Record the INITIAL state at gpud BEFORE the first present flush
        // (the gpud queue is sequential) - the window composites from
        // transparent instead of flashing full-on for one frame, and a
        // stale override from the slot's previous tenant is replaced.
        self.send_layer_transform(idx);
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
        let layer = Self::wt_layer(idx);
        let now = nexus_abi::nsec().unwrap_or(0);
        self.animation_driver.reset_clock(now);
        self.animation_driver.spring_to(layer, AnimProp::Opacity, cur.opacity, 0.0, EXIT_SPRING);
        self.animation_driver.spring_to(layer, AnimProp::ScaleX, cur.scale, CLOSE_SCALE_TO, EXIT_SPRING);
        let _ = debug_println("windowd: transition close");
    }

    /// Center of the dock cell the window will occupy AFTER minimizing: it is
    /// appended to the minimized list, so its slot index is the current count
    /// and the bar is laid out for count+1 icons (exact dock geometry SSOT —
    /// not the old fixed bottom-center approximation).
    fn dock_cell_center_after_minimize(&self) -> (f32, f32) {
        let (_, n) = self.windows.minimized_list();
        let bar = crate::dock::dock_rect(self.mode.width, self.mode.height, n + 1);
        let cell = crate::dock::dock_slot_rect(bar, n);
        (
            cell.x as f32 + cell.width as f32 / 2.0,
            cell.y as f32 + cell.height as f32 / 2.0,
        )
    }

    /// MINIMIZE transition: fly toward the window's own future dock cell
    /// (translate + shrink + fade), then the deferred `minimize_window` runs
    /// on convergence.
    pub(super) fn start_minimize_transition(&mut self, idx: usize) {
        if self.apps[idx].pending_wm.is_some() {
            return;
        }
        let cur = self.apps[idx].transform;
        self.apps[idx].transform.active = true;
        self.apps[idx].pending_wm = Some(PendingWm::Minimize);
        // Fly target: the exact dock cell, as a delta from the window center
        // (the layer scale is CENTER-anchored in gpud — center-to-center math).
        let (cell_cx, cell_cy) = self.dock_cell_center_after_minimize();
        let win = &self.apps[idx].win;
        let target_x = cell_cx - (win.x as f32 + win.w as f32 / 2.0);
        let target_y = cell_cy - (win.y as f32 + win.h as f32 / 2.0);
        let layer = Self::wt_layer(idx);
        let now = nexus_abi::nsec().unwrap_or(0);
        self.animation_driver.reset_clock(now);
        self.animation_driver.spring_to(layer, AnimProp::TranslateX, cur.dx, target_x, EXIT_SPRING);
        self.animation_driver.spring_to(layer, AnimProp::TranslateY, cur.dy, target_y, EXIT_SPRING);
        self.animation_driver.spring_to(layer, AnimProp::Opacity, cur.opacity, 0.15, EXIT_SPRING);
        self.animation_driver.spring_to(layer, AnimProp::ScaleX, cur.scale, MIN_SCALE_TO, EXIT_SPRING);
        let _ = debug_println("windowd: transition minimize");
    }

    /// RESTORE transition: the window flies IN from its dock cell (the exact
    /// reverse of minimize). Unlike close/minimize the WM state change runs UP
    /// FRONT (`restore_window` re-mounts + raises + focuses), then the springs
    /// carry the transform from the dock origin to identity — convergence
    /// falls into `finish_window_transitions`' `None` arm (settle at
    /// identity), no `PendingWm` needed.
    pub(super) fn start_restore_transition(&mut self, id: crate::window_scene::WindowId, from_cx: f32, from_cy: f32) {
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
        // Pre-seed the dock-origin state at gpud before the restore's queued
        // present composites (the gpud queue is sequential) — no full-size
        // flash before the first spring tick.
        self.send_layer_transform(idx);
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
            return; // geometry did not change — nothing to animate
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
        // Seed before the toggle's queued full present (sequential gpud queue).
        self.send_layer_transform(idx);
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
        self.send_layer_transform(idx);
        true
    }

    /// Emit the slot's CURRENT transform as one `OP_SET_LAYER_TRANSFORM`
    /// (fire-and-forget; gpud records + coalesces — never present-per-op).
    pub(super) fn send_layer_transform(&mut self, idx: usize) {
        let t = self.apps[idx].transform;
        let frame = nexus_display_proto::encode_set_layer_transform(
            (idx as u32) + 1,
            t.dx as i16,
            t.dy as i16,
            (t.opacity * 255.0).clamp(0.0, 255.0) as u8,
            (t.scale * 100.0).clamp(1.0, 400.0) as u16,
        );
        let _ = self.send_gpud_fire_forget(&frame);
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
                    self.send_layer_transform(idx);
                }
                None => {
                    // Open settled: one final exact-identity override.
                    self.send_layer_transform(idx);
                }
            }
        }
    }
}
