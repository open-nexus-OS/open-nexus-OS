// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! BOUNDARY: this file is the FLOATING app-window slice of the app-surface service: show and
//! close the window, the CPU executor's blur cache, the window's geometry. The window's content
//! is the app's own buffer (RFC-0114): nothing here moves a pixel. It MUST NOT grow
//! window-chrome/sizing/decoration logic — that is the `ui/widgets/window` widget's job (see
//! `app_window.rs` header).
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! ADR: docs/adr/0092-a-surface-is-a-buffer-the-app-draws-and-the-gpu-reads-gpud-allocates-it-for-its-owner-windowd-composes-by-reference.md

use super::app_window::nexus_abi_cap_close;
use super::*;

impl DisplayServerRuntime {
    /// Shows the window of slot `idx`: its role's z-band, focus (an overlay never steals it),
    /// and — where the CPU executor composes — its blur cache (best-effort: without one the
    /// glass is blurred live). The content is the app's buffer, drawn from its first present.
    pub(super) fn open_app_window(&mut self, idx: usize) {
        let blurs_itself = self.app_presentation(idx).full_screen || self.gpu_composes;
        if self.apps[idx].win.blur_cache.is_none() && !blurs_itself {
            let (w, h) = (self.apps[idx].win.w, self.apps[idx].win.h);
            let cache = self.atlas_alloc.alloc(w, h);
            self.apps[idx].win.set_blur_cache(cache);
        }
        self.apps[idx].win.visible = true;
        let wid = crate::window_scene::WindowId::App(idx as u16);
        let presentation = self.app_presentation(idx);
        // The surface's DECLARED level resolves its z-band (overlay = above all floating
        // windows); an overlay never steals window focus.
        self.windows.set_role(wid, presentation.role);
        if presentation.role == crate::window_scene::WindowRole::Overlay {
            self.windows.show_unfocused(wid);
        } else {
            self.show_window(wid);
        }
        let rect = self.app_window_rect(idx);
        self.queue_dirty_rect(rect);
    }

    /// Reset everything derived from THE SURFACE when a client (re)creates one: its declared
    /// layers (RFC-0109) — regions, scroll layers, washes. All of it is surface-local, so none
    /// of it may survive the surface it describes — a resize is a destroy + create, and the stale
    /// half is exactly what painted the file manager's panes at their old size in the corner of a
    /// maximized window. Fail-closed: a surface has no layers until it declares some.
    pub(super) fn reset_surface_state(&mut self, idx: usize) {
        self.apps[idx].layers.clear();
    }

    /// Closes slot `idx`'s window: `on_purpose` (the user, the window manager) tells the app
    /// before its channel goes (RFC-0112); a dead client's channel just goes.
    pub(super) fn close_app_window(&mut self, idx: usize, on_purpose: bool) {
        let vacated = self.app_window_rect(idx);
        self.apps[idx].starting = false;
        self.drop_solid(idx);
        self.apps[idx].win.visible = false;
        self.hide_window(crate::window_scene::WindowId::App(idx as u16));
        self.apps[idx].win.end_drag();
        // A closed window gates nobody (TASK-0074 D4) and holds no tile (TASK-0066).
        self.apps[idx].reset_wm_state();
        if let Some(cache) = self.apps[idx].win.take_blur_cache() {
            self.atlas_alloc.free(cache);
        }
        self.queue_dirty_rect(vacated);
        self.sync_surface_focus_to_imed();
        // The backdrop blur is destination-so-far: any window whose cached
        // blur was captured while the closed window overlapped it keeps a
        // GHOST of it under the glass. Invalidate every intersecting window's
        // blur cache (GPU re-blur next composite — cheap).
        self.invalidate_overlapping_blur(vacated, idx);
        // W4 slot recycling: a user-close ENDS the binding — the surface (and one it was
        // replacing) goes, its buffer back to gpud (RFC-0114: unbound once the last frame that
        // read it retired), the event channel is released (cap + nonce table entry), and the
        // slot is reset so the NEXT launch reuses it. The parked app process itself is the open
        // reaper (#29); it costs zero CPU (its frame pulses are withheld, its presents refused).
        for id in
            [self.apps[idx].surface_id.take(), self.apps[idx].retiring.take()].into_iter().flatten()
        {
            self.drop_surface(id);
        }
        // RFC-0086: the window left the set — the shell drops its tile marker.
        self.push_window_set();
        #[cfg(nexus_env = "os")]
        if let Some(ch) = self.apps[idx].event_channel.take() {
            if let Some(pos) = self.event_channels.iter().position(|e| e.1 == ch) {
                // The table keeps its order; the snapshot tracker moves with it.
                self.event_channels.remove(pos);
                self.presentation.forget(pos);
            }
            // What waited for the closed window's client goes with its channel (ADR-0075).
            if on_purpose {
                self.close_client_channel(ch);
            } else {
                self.forget_client_channel(ch);
                let _ = nexus_abi_cap_close(ch);
            }
        }
        self.apps[idx].frame_pulse_pending = false;
        self.reset_surface_state(idx);
        // The slot's transform returns to the identity; its next tenant takes a fresh key, so no
        // frame can lend it the faded-out state of this one (RFC-0114 §7).
        self.apps[idx].transform = WinTransform::IDENTITY;
        self.apps[idx].pending_wm = None;
        self.apps[idx].drag_offset = None;
        self.owe_frame();
        let _ = debug_println(&alloc::format!("WINDOWD: app window closed slot={idx}"));
    }

    /// Invalidate the cached backdrop blur of every OTHER visible window
    /// intersecting `rect` — their destination-so-far blur may hold a ghost
    /// of content that just vanished/moved there (close, minimize).
    pub(super) fn invalidate_overlapping_blur(&mut self, rect: DamageRect, skip_idx: usize) {
        for i in 0..self.apps.len() {
            if i == skip_idx || !self.apps[i].win.visible {
                continue;
            }
            let r = self.app_window_rect(i);
            let intersects = rect.x < r.x + r.width
                && rect.x + rect.width > r.x
                && rect.y < r.y + r.height
                && rect.y + rect.height > r.y;
            if intersects {
                self.apps[i].win.blur_valid = false;
            }
        }
    }

    /// The atlas's deepest row ever reached, its rows in use and its rows (TASK-0333 6b-2 P0:
    /// what a lazy atlas holds committed, ADR-0078).
    pub(crate) fn atlas_rows(&self) -> (u32, u32, u32) {
        let a = &self.atlas_alloc;
        (a.peak_rows(), a.used_rows(), a.capacity())
    }

    /// What the grant gives the runtime, once at start: whether the GPU composes (TASK-0340)
    /// and the atlas's rows in the granted object (ADR-0078) — the atlas grows to them.
    pub(crate) fn take_grant(&mut self, gpu_composes: bool, atlas_rows: u32) {
        self.set_gpu_composes(gpu_composes);
        self.atlas_alloc.grow(atlas_rows);
    }

    pub(super) fn app_window_rect(&self, idx: usize) -> DamageRect {
        self.apps[idx].win.damage_rect(self.mode.width, self.mode.height)
    }
}
