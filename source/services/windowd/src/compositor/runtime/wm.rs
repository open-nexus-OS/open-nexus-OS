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
use crate::dock;
use crate::compositor::shell_window::{Frame, ResizeEdge};
use crate::snap;
use crate::window_scene::WindowId;

/// Minimum resizable window size: the three title buttons + a label sliver
/// wide, the title bar + a few content rows tall.
const MIN_WIN_W: u32 = 3 * 48 + 60;
const MIN_WIN_H: u32 = 120;

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
            WindowId::Settings => self.settings_win.end_drag(),
            WindowId::DslDemo => self.dsl_win.end_drag(),
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
            WindowId::Settings => self.settings_win.blur_valid = false,
            WindowId::DslDemo => self.dsl_win.blur_valid = false,
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
        // The Settings panel is a fixed-size static window (its atlas surface
        // can't cover the display), so its "□" is a no-op — never fullscreen it.
        if matches!(id, WindowId::Settings) {
            return;
        }
        let (mode_w, mode_h) = (self.mode.width, self.mode.height);
        if self.windows.is_fullscreen(id) {
            match id {
                WindowId::Chat => self.chat.leave_fullscreen(),
                WindowId::Settings | WindowId::DslDemo => {}
                WindowId::Search => {
                    self.search.leave_fullscreen();
                    // Shrink the pool surfaces back to the floating size.
                    let (w, h) = (self.search.w, self.search.h);
                    let _ = self.ensure_search_surfaces(w, h);
                    self.search_set_extent();
                    self.commit_search_scroll_position();
                    self.search.surface_dirty = true;
                }
            }
            self.windows.set_fullscreen(id, false);
            let _ =
                debug_println(&alloc::format!("windowd: unfullscreen id={}", Self::window_name(id)));
        } else {
            match id {
                WindowId::Chat => {
                    // The full-width chat band backs any h ≤ its height.
                    let band_h = self.chat.atlas.map(|s| s.height).unwrap_or(mode_h);
                    self.chat.enter_fullscreen(mode_w, mode_h.min(band_h));
                }
                WindowId::Settings | WindowId::DslDemo => {}
                WindowId::Search => {
                    // TRUE fullscreen needs pool surfaces at display size; if
                    // the pool can't back them the toggle is refused honestly.
                    if !self.ensure_search_surfaces(mode_w, mode_h) {
                        let _ = debug_println("windowd: fullscreen denied (search pool)");
                        return;
                    }
                    self.search.enter_fullscreen(mode_w, mode_h);
                    self.search_set_extent();
                    self.commit_search_scroll_position();
                    self.search.surface_dirty = true;
                }
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
            WindowId::Settings => self.settings_win.contains(cx, cy),
            WindowId::DslDemo => self.dsl_win.contains(cx, cy),
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
        let settings_hover = want(WindowId::Settings, &self.settings_win);
        if settings_hover != self.settings_win.title_hover {
            self.settings_win.title_hover = settings_hover;
            self.settings_win.surface_dirty = true;
            self.queue_dirty_rect(self.settings_window_rect());
        }
    }

    // ── Edge/corner resize + drag-to-edge snap (TASK-0070 Phase 3) ──

    /// Begin an edge-resize drag: remember the grabbed edge, the START frame
    /// (the math is deterministic in it) and the grab point.
    pub(super) fn begin_window_resize(&mut self, id: WindowId, edge: ResizeEdge, cx: i32, cy: i32) {
        let start = match id {
            WindowId::Chat => self.chat.frame(),
            WindowId::Search => self.search.frame(),
            WindowId::Settings => self.settings_win.frame(),
            WindowId::DslDemo => self.dsl_win.frame(),
        };
        self.raise_window(id);
        self.resize_drag = Some((id, edge, start, (cx, cy)));
        self.set_cursor_shape(cursor::CursorShape::for_edge(edge));
    }

    /// Continue an active edge-resize drag: recompute the frame from the drag
    /// START (no incremental drift), clamp to min size + display, apply.
    pub(super) fn apply_window_resize(&mut self, cx: i32, cy: i32) {
        let Some((id, edge, start, (gx, gy))) = self.resize_drag else {
            return;
        };
        let frame = Frame::resized(
            start,
            edge,
            cx - gx,
            cy - gy,
            MIN_WIN_W,
            MIN_WIN_H,
            self.mode.width,
            self.mode.height,
        );
        let current = match id {
            WindowId::Chat => self.chat.frame(),
            WindowId::Search => self.search.frame(),
            WindowId::Settings => self.settings_win.frame(),
            WindowId::DslDemo => self.dsl_win.frame(),
        };
        if frame != current {
            self.apply_window_frame(id, frame.x, frame.y, frame.w, frame.h);
        }
    }

    /// End an edge-resize drag (pointer release): one honest size marker.
    pub(super) fn end_window_resize(&mut self) {
        if let Some((id, _, _, _)) = self.resize_drag.take() {
            let (w, h) = match id {
                WindowId::Chat => (self.chat.w, self.chat.h),
                WindowId::Search => (self.search.w, self.search.h),
                WindowId::Settings => (self.settings_win.w, self.settings_win.h),
                WindowId::DslDemo => (self.dsl_win.w, self.dsl_win.h),
            };
            let _ = debug_println(&alloc::format!(
                "windowd: resize id={} w={w} h={h}",
                Self::window_name(id)
            ));
        }
    }

    /// A title-bar drag released with the pointer at a display edge snaps the
    /// window (left/right half, top = fullscreen). Returns true when a snap
    /// was applied (the caller skips the plain drag release).
    pub(super) fn apply_release_snap(&mut self, id: WindowId, cx: i32, cy: i32) -> bool {
        let Some(target) = snap::snap_target_at(cx, cy, self.mode.width) else {
            return false;
        };
        match target {
            snap::SnapTarget::Fullscreen => {
                // Route through the fullscreen toggle: chrome-cover + restore
                // semantics live in ONE place.
                if !self.windows.is_fullscreen(id) {
                    self.toggle_fullscreen(id);
                }
                let _ = debug_println(&alloc::format!(
                    "windowd: snap edge=top id={}",
                    Self::window_name(id)
                ));
            }
            half => {
                let (x, y, w, h) = snap::snap_frame(half, self.mode.width, self.mode.height);
                self.apply_window_frame(id, x, y, w, h);
                let _ = debug_println(&alloc::format!(
                    "windowd: snap edge={} id={}",
                    if half == snap::SnapTarget::LeftHalf { "left" } else { "right" },
                    Self::window_name(id)
                ));
            }
        }
        true
    }

    /// Apply a display-space frame to a window: damage the vacated region,
    /// resize the content surfaces (search re-mounts from the pool; the chat
    /// band is boot-reserved and only clamps), re-render, damage the new
    /// region. The SINGLE geometry-apply path shared by edge resize, snap
    /// halves, and the fullscreen toggle.
    pub(super) fn apply_window_frame(&mut self, id: WindowId, x: i32, y: i32, w: u32, h: u32) {
        let old = self.window_damage_rect(id);
        self.queue_gpu_blit_rect(old);
        match id {
            WindowId::Chat => {
                // The chat band is full-width and CHAT_PANEL_H+CHAT_OVERSCAN
                // tall — clamp the frame to what the band can back.
                let band_h = self.chat.atlas.map(|s| s.height).unwrap_or(h);
                let w = w.min(self.mode.width);
                let h = h.min(band_h);
                self.chat.set_frame(x, y, w, h);
            }
            WindowId::Search => {
                let w = w.min(self.mode.width);
                let h = h.min(self.mode.height);
                if !self.ensure_search_surfaces(w, h) {
                    // Pool exhausted at this size: keep the old frame (the
                    // window must never show garbage or vanish).
                    let _ = debug_println("windowd: resize denied (search pool)");
                    return;
                }
                self.search.set_frame(x, y, w, h);
                self.search_set_extent();
                self.commit_search_scroll_position();
                self.search.surface_dirty = true;
            }
            WindowId::Settings => {
                // Static panel — clamp to its fixed atlas band; the content does
                // not reflow, so a resize just changes the glass frame.
                let band_h = self.settings_win.atlas.map(|s| s.height).unwrap_or(h);
                let w = w.min(self.mode.width);
                let h = h.min(band_h);
                self.settings_win.set_frame(x, y, w, h);
                self.settings_win.surface_dirty = true;
            }
            WindowId::DslDemo => {
                // Interpreter body — clamp to the atlas band; a resize keeps
                // the current layout (re-layout on open/interaction only).
                let band_h = self.dsl_win.atlas.map(|s| s.height).unwrap_or(h);
                let w = w.min(self.mode.width);
                let h = h.min(band_h);
                self.dsl_win.set_frame(x, y, w, h);
                self.dsl_win.surface_dirty = true;
            }
        }
        let new = self.window_damage_rect(id);
        self.queue_gpu_blit_rect(new);
    }

    /// Ensure the search pool surfaces can back a `w`×`h` window: grow-realloc
    /// when too small, lazily shrink when 2× oversized, keep otherwise. The
    /// blur cache follows best-effort (missing blur = unblurred glass).
    /// TRANSACTIONAL: the old surfaces are only freed once a replacement is
    /// secured (or re-secured at the old size), so the window never loses its
    /// content surface mid-resize.
    fn ensure_search_surfaces(&mut self, w: u32, h: u32) -> bool {
        let fits = |s: crate::atlas::AtlasSurface| {
            s.width >= w
                && s.height >= h
                && (s.width as u64 * s.height as u64) <= 2 * (w as u64 * h as u64).max(1)
        };
        let Some(old_content) = self.search.atlas else {
            return false; // unmounted (hidden) — nothing to resize
        };
        if fits(old_content) && self.search.blur_cache.map(fits).unwrap_or(false) {
            return true;
        }
        // Fast path: the pool has room beside the old surfaces.
        if let Some(content) = self.atlas_alloc.alloc(w, h) {
            let blur = self.atlas_alloc.alloc(w, h);
            if let Some((old, old_blur)) = self.search.unmount() {
                self.atlas_alloc.free(old);
                if let Some(old_blur) = old_blur {
                    self.atlas_alloc.free(old_blur);
                }
            }
            self.search.mount(content, blur);
            return true;
        }
        // Tight pool: free the old surfaces first, then retry; on failure fall
        // back to remounting at the OLD capacity (just freed → succeeds).
        let (old_w, old_h) = (old_content.width, old_content.height);
        if let Some((old, old_blur)) = self.search.unmount() {
            self.atlas_alloc.free(old);
            if let Some(old_blur) = old_blur {
                self.atlas_alloc.free(old_blur);
            }
        }
        if let Some(content) = self.atlas_alloc.alloc(w, h) {
            let blur = self.atlas_alloc.alloc(w, h);
            self.search.mount(content, blur);
            return true;
        }
        if let Some(content) = self.atlas_alloc.alloc(old_w, old_h) {
            let blur = self.atlas_alloc.alloc(old_w, old_h);
            self.search.mount(content, blur);
        }
        false
    }

    /// Select the pointer shape for the current pointer position: an ACTIVE
    /// resize drag pins its edge shape; otherwise the topmost floating
    /// window's border band under the cursor picks one; else default.
    pub(super) fn update_cursor_shape_for_pointer(&mut self, cx: i32, cy: i32) {
        let shape = if let Some((_, edge, _, _)) = self.resize_drag {
            cursor::CursorShape::for_edge(edge)
        } else {
            let (hit, n) = self.windows.hit_order(USE_DESKTOP_SHELL);
            let mut shape = cursor::CursorShape::Default;
            for &wid in &hit[..n] {
                let frame = match wid {
                    WindowId::Chat => self.chat.frame(),
                    WindowId::Search => self.search.frame(),
                    WindowId::Settings => self.settings_win.frame(),
                    WindowId::DslDemo => self.dsl_win.frame(),
                };
                if frame.contains(cx, cy) {
                    if !self.windows.is_fullscreen(wid) {
                        if let Some(edge) = frame.resize_edge_at(cx, cy) {
                            shape = cursor::CursorShape::for_edge(edge);
                        }
                    }
                    break; // topmost window under the pointer decides
                }
            }
            shape
        };
        self.set_cursor_shape(shape);
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
        // `BAR_TINT[3]` is the SSOT for the frosted alpha; the theme swaps the RGB.
        const BAR_TINT: [u8; 4] = [56, 50, 46, 150];
        let tk = self.theme();
        let bar_col = crate::theme::with_alpha(tk.glass_tint, BAR_TINT[3]);
        let glyph_tint = Some(crate::theme::rgb3(tk.fg));
        let band = &mut self.band_scratch;
        for ly in 0..dock::DOCK_H {
            let row = &mut band[0..stride];
            row[..row_bytes].fill(0);
            super::super::shell_window::write_tint_span(row, 0, bar_local.width, bar_col);
            for (slot, &wid) in list[..n].iter().enumerate() {
                let cell = dock::dock_slot_rect(bar_local, slot);
                let (icon, dim) = match wid {
                    WindowId::Chat => {
                        (crate::assets::DOCK_CHAT_ICON_BGRA, crate::assets::DOCK_CHAT_ICON_DIM)
                    }
                    WindowId::Search => {
                        (crate::assets::DOCK_SEARCH_ICON_BGRA, crate::assets::DOCK_SEARCH_ICON_DIM)
                    }
                    // DSL demo reuses the search glyph until its own is baked.
                    WindowId::DslDemo => {
                        (crate::assets::DOCK_SEARCH_ICON_BGRA, crate::assets::DOCK_SEARCH_ICON_DIM)
                    }
                    // Placeholder dock glyph until a gear icon is baked (Phase 10).
                    WindowId::Settings => {
                        (crate::assets::MENU_ICON_BGRA, crate::assets::MENU_ICON_DIM)
                    }
                };
                let iy0 = cell.y + cell.height.saturating_sub(dim) / 2;
                if ly >= iy0 && ly < iy0 + dim {
                    let ix = cell.x + cell.width.saturating_sub(dim) / 2;
                    super::desktop_layer::blend_icon_row(row, ix, icon, dim, ly - iy0, 255, glyph_tint);
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
