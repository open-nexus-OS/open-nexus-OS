// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: windowd compositor runtime — window-management actions (TASK-0070
//! Phase 2): minimize into the dock, restore, fullscreen toggle, and the dock
//! surface lifecycle. The DECISIONS live in the host-tested `window_scene`
//! stack and `compositor/dock` geometry; this module only applies them to the
//! runtime (surfaces, damage, markers).
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: No tests (pure logic host-tested in `window_scene` + `dock`)

use super::*;
use crate::compositor::dock;
use crate::window_scene::WindowId;

impl DisplayServerRuntime {
    /// Minimize a window into the dock. Refused (with a marker) when the dock
    /// surface cannot be allocated — a window must NEVER become unreachable.
    pub(super) fn minimize_window(&mut self, id: WindowId) {
        if !self.windows.is_visible(id) || self.windows.is_minimized(id) {
            return;
        }
        if !self.ensure_dock_surface() {
            let _ = debug_println("windowd: minimize denied (dock atlas)");
            return;
        }
        let vacated = self.window_damage_rect(id);
        match id {
            WindowId::Chat => self.chat.end_drag(),
            WindowId::Search => self.search.end_drag(),
        }
        self.windows.minimize(id);
        let _ = debug_println(&alloc::format!("windowd: minimize id={}", Self::window_name(id)));
        self.queue_gpu_blit_rect(vacated);
        self.update_dock();
    }

    /// Restore a minimized window from the dock: composited again, raised,
    /// focused (blur cache invalidated — the backdrop may have changed).
    pub(super) fn restore_window(&mut self, id: WindowId) {
        if !self.windows.is_minimized(id) {
            return;
        }
        self.windows.restore(id);
        match id {
            WindowId::Chat => self.chat.blur_valid = false,
            WindowId::Search => self.search.blur_valid = false,
        }
        let _ = debug_println(&alloc::format!("windowd: restore id={}", Self::window_name(id)));
        let rect = self.window_damage_rect(id);
        self.queue_gpu_blit_rect(rect);
        self.update_dock();
        // Restoring a fullscreen window re-covers the chrome — full present.
        if self.windows.fullscreen_active().is_some() {
            self.queue_full_frame_damage();
        }
    }

    /// Toggle fullscreen on a window (the title-bar "□"). Fullscreen covers
    /// the chrome (`chrome_composited` gates on `fullscreen_active`); leaving
    /// restores the remembered floating origin.
    pub(super) fn toggle_fullscreen(&mut self, id: WindowId) {
        let (mode_w, mode_h) = (self.mode.width, self.mode.height);
        if self.windows.is_fullscreen(id) {
            match id {
                WindowId::Chat => self.chat.leave_fullscreen(),
                WindowId::Search => self.search.leave_fullscreen(),
            }
            self.windows.set_fullscreen(id, false);
            let _ =
                debug_println(&alloc::format!("windowd: unfullscreen id={}", Self::window_name(id)));
        } else {
            match id {
                WindowId::Chat => self.chat.enter_fullscreen(mode_w, mode_h),
                WindowId::Search => self.search.enter_fullscreen(mode_w, mode_h),
            }
            self.windows.set_fullscreen(id, true);
            let _ =
                debug_println(&alloc::format!("windowd: fullscreen id={}", Self::window_name(id)));
        }
        // Chrome visibility + window geometry both changed → full present.
        self.queue_full_frame_damage();
    }

    /// Whole-display damage (chrome appears/disappears, fullscreen toggles).
    pub(super) fn queue_full_frame_damage(&mut self) {
        self.queue_gpu_blit_rect(DamageRect {
            x: 0,
            y: 0,
            width: self.mode.width,
            height: self.mode.height,
        });
    }

    /// Title-bar button hover `[– □ ×]`: only the TOPMOST window under the
    /// cursor may show a hover (same front-to-back order as presses); every
    /// other window's hover clears. Changes re-render that window's surface.
    pub(super) fn update_title_hovers(&mut self, cx: i32, cy: i32) {
        use crate::compositor::shell_window::TitleButton;
        let (hit, n) = self.windows.hit_order(USE_DESKTOP_SHELL);
        let owner = hit[..n].iter().copied().find(|&wid| match wid {
            WindowId::Chat => self.chat.contains(cx, cy),
            WindowId::Search => self.search.contains(cx, cy),
        });
        let want = |wid: WindowId, win: &super::super::shell_window::ShellWindow| -> Option<TitleButton> {
            if owner == Some(wid) {
                win.title_button_at(cx, cy)
            } else {
                None
            }
        };
        let search_hover = want(WindowId::Search, &self.search);
        if search_hover != self.search.title_hover {
            self.search.title_hover = search_hover;
            self.search.surface_dirty = true;
            self.queue_dirty_rect(self.search_window_rect());
        }
        let chat_hover = want(WindowId::Chat, &self.chat);
        if chat_hover != self.chat.title_hover {
            self.chat.title_hover = chat_hover;
            self.chat.surface_dirty = true;
            let rect = self.chat.damage_rect(self.mode.width, self.mode.height);
            self.queue_gpu_blit_rect(rect);
        }
    }

    // ── Dock (bottom-center bar of minimized windows) ──

    /// The dock's display rect while it is active (≥1 minimized window, no
    /// greeter, no fullscreen cover). `None` = no dock on screen.
    pub(super) fn dock_bar_rect(&self) -> Option<dock::DockRect> {
        let n = self.windows.minimized_list().1;
        if n == 0
            || self.dock_surface.is_none()
            || self.greeter_active()
            || !self.session_resolved()
            || self.windows.fullscreen_active().is_some()
        {
            return None;
        }
        Some(dock::dock_rect(self.mode.width, self.mode.height, n))
    }

    /// Allocate the dock's atlas surface on first use (sized for the MAX icon
    /// count so a later minimize never re-allocates). False = pool exhausted.
    fn ensure_dock_surface(&mut self) -> bool {
        if self.dock_surface.is_some() {
            return true;
        }
        let w = dock::dock_width(crate::window_scene::MAX_WINDOWS);
        match self.atlas_alloc.alloc(w, dock::DOCK_H) {
            Some(surface) => {
                self.dock_surface = Some(surface);
                self.dock_dirty = true;
                true
            }
            None => false,
        }
    }

    /// Reconcile the dock with the stack after a minimize/restore/close: frees
    /// the surface when the last window left, re-renders + damages on change.
    pub(super) fn update_dock(&mut self) {
        let n = self.windows.minimized_list().1;
        // Damage the WIDEST footprint the bar had/has so shrink leaves no trail.
        let widest = self.dock_rendered_n.max(n);
        if widest > 0 {
            let bar = dock::dock_rect(self.mode.width, self.mode.height, widest);
            self.queue_gpu_blit_rect(DamageRect {
                x: bar.x,
                y: bar.y,
                width: bar.width,
                height: bar.height,
            });
        }
        if n == 0 {
            if let Some(surface) = self.dock_surface.take() {
                self.atlas_alloc.free(surface);
                let _ = debug_println("windowd: dock hide");
            }
            self.dock_rendered_n = 0;
            self.dock_dirty = false;
            return;
        }
        if n != self.dock_rendered_n {
            let _ = debug_println(&alloc::format!("windowd: dock show (n={n})"));
            self.dock_dirty = true;
        }
    }

    /// Render the dock surface (bar tint + one icon per minimized window, in
    /// the stack's stable dock order). 2D-packed like the search window: the
    /// surface may sit at a column offset, so rows write `w*4` bytes at it.
    pub(super) fn render_dock_surface(&mut self) -> Result<(), WindowdError> {
        let Some(handle) = self.framebuffer else {
            return Ok(());
        };
        let Some(surface) = self.dock_surface else {
            return Ok(());
        };
        let (list, n) = self.windows.minimized_list();
        if n == 0 {
            return Ok(());
        }
        let stride = self.mode.stride as usize;
        let w = dock::dock_width(crate::window_scene::MAX_WINDOWS);
        let row_bytes = w as usize * 4;
        if self.band_scratch.len() < stride {
            return Err(WindowdError::BufferLengthMismatch);
        }
        // Bar-local geometry: slots relative to a bar at (0, 0).
        let bar_local =
            dock::DockRect { x: 0, y: 0, width: dock::dock_width(n), height: dock::DOCK_H };
        // Translucent glass tint; the composite adds blur + corners + shadow.
        const BAR_TINT: [u8; 4] = [56, 50, 46, 150];
        let band = &mut self.band_scratch;
        for ly in 0..dock::DOCK_H {
            let row = &mut band[0..stride];
            row[..row_bytes].fill(0);
            super::super::shell_window::write_tint_span(row, 0, bar_local.width, BAR_TINT);
            for (slot, &wid) in list[..n].iter().enumerate() {
                let cell = dock::dock_slot_rect(bar_local, slot);
                let (icon, dim) = match wid {
                    WindowId::Chat => {
                        (crate::assets::DOCK_CHAT_ICON_BGRA, crate::assets::DOCK_CHAT_ICON_DIM)
                    }
                    WindowId::Search => {
                        (crate::assets::DOCK_SEARCH_ICON_BGRA, crate::assets::DOCK_SEARCH_ICON_DIM)
                    }
                };
                let iy0 = cell.y + cell.height.saturating_sub(dim) / 2;
                if ly >= iy0 && ly < iy0 + dim {
                    let ix = cell.x + cell.width.saturating_sub(dim) / 2;
                    super::desktop_layer::blend_icon_row(row, ix, icon, dim, ly - iy0, 255);
                }
            }
            let dst = (surface.abs_row + ly) as usize * stride + surface.x as usize * 4;
            vmo_write(handle, dst, &row[..row_bytes])
                .map_err(|_| WindowdError::BufferLengthMismatch)?;
        }
        self.dock_rendered_n = n;
        Ok(())
    }
}
