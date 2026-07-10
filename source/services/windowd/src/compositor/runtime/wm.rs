// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: windowd compositor runtime — window-management actions (TASK-0070
//! Phase 2): minimize into the dock, restore, fullscreen toggle, and the dock
//! surface lifecycle. The DECISIONS live in the host-tested `window_scene`
//! stack and `compositor/dock` geometry; this module only applies them to the
//! runtime (surfaces, damage, markers). Post-cleanup (cleanup-map DELETE):
//! the only windows are the app-client (floating) and the desktop base — the
//! legacy chat/search/settings windows are DSL apps now.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: No tests (pure logic host-tested in `window_scene` + `dock`)

use super::*;
use crate::compositor::shell_window::{Frame, ResizeEdge};
use crate::dock;
use crate::snap;
use crate::window_scene::WindowId;

/// Minimum resizable window size: the three title buttons + a label sliver
/// wide, the title bar + a few content rows tall.
const MIN_WIN_W: u32 = 3 * 48 + 60;
const MIN_WIN_H: u32 = 120;

impl DisplayServerRuntime {
    /// The window's current display-space frame. The desktop base is always
    /// the full display (its geometry follows the mode; never WM-changed).
    fn window_frame(&self, id: WindowId) -> Frame {
        match id {
            WindowId::AppClient => self.app_win.frame(),
            _ => Frame {
                x: 0,
                y: 0,
                w: self.mode.width,
                h: self.mode.height,
                title_h: 0,
                close_w: 0,
            },
        }
    }

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
        if id == WindowId::AppClient {
            self.app_win.end_drag();
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
        if id == WindowId::AppClient {
            self.app_win.blur_valid = false;
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
        if id != WindowId::AppClient {
            return; // the desktop base is never fullscreen-toggled
        }
        let (mode_w, mode_h) = (self.mode.width, self.mode.height);
        if self.windows.is_fullscreen(id) {
            // Restore the floating frame + chrome height (fullscreen zeroed
            // `title_h`). Restoring it here — not waiting for the re-create —
            // keeps `push_app_content_rect`'s title inset correct, so the
            // window doesn't grow by `title_h` each fullscreen round-trip.
            self.app_win.leave_fullscreen();
            self.app_win.title_h = self.app_title_h();
            self.windows.set_fullscreen(id, false);
            let _ =
                debug_println(&alloc::format!("windowd: unfullscreen id={}", Self::window_name(id)));
        } else {
            // Cover the display; the OP_SURFACE_RECT push below makes the app
            // re-create its surface at display size (the atlas band is
            // reallocated on that re-create — no display-sized band held for a
            // floating window). The client re-render owns the pixels.
            self.app_win.enter_fullscreen(mode_w, mode_h);
            self.windows.set_fullscreen(id, true);
            let _ =
                debug_println(&alloc::format!("windowd: fullscreen id={}", Self::window_name(id)));
        }
        // After the fullscreen flag settles (enter or leave), push the new
        // content rect so the app re-renders at the new size
        // (`push_app_content_rect` reads the flag → full display vs. inset).
        self.push_app_content_rect();
        // Title stays sharp at the new frame width while the band re-creates.
        self.update_app_title_overlay();
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
            WindowId::AppClient => self.app_win.contains(cx, cy),
            // The desktop base has no window chrome to grab — clicks fall
            // through to the shell's own surface (client input), never to a
            // window drag/hit owner.
            _ => false,
        });
        let app_hover: Option<TitleButton> = if owner == Some(WindowId::AppClient) {
            self.app_win.title_button_at(cx, cy)
        } else {
            None
        };
        if app_hover != self.app_win.title_hover {
            self.app_win.title_hover = app_hover;
            self.app_win.surface_dirty = true;
            self.queue_dirty_rect(self.app_window_rect());
        }
    }

    // ── Edge/corner resize + drag-to-edge snap (TASK-0070 Phase 3) ──

    /// Begin an edge-resize drag: remember the grabbed edge, the START frame
    /// (the math is deterministic in it) and the grab point.
    pub(super) fn begin_window_resize(&mut self, id: WindowId, edge: ResizeEdge, cx: i32, cy: i32) {
        let start = self.window_frame(id);
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
        if frame != self.window_frame(id) {
            self.apply_window_frame(id, frame.x, frame.y, frame.w, frame.h);
        }
    }

    /// End an edge-resize drag (pointer release): one honest size marker.
    pub(super) fn end_window_resize(&mut self) {
        if let Some((id, _, _, _)) = self.resize_drag.take() {
            let frame = self.window_frame(id);
            let _ = debug_println(&alloc::format!(
                "windowd: resize id={} w={} h={}",
                Self::window_name(id),
                frame.w,
                frame.h
            ));
            // Resize negotiation: the client surface keeps its created size, so
            // on release tell the app its new CONTENT rect (window minus the
            // title bar). It re-creates its surface at that size, which
            // re-allocs the band on the fresh SURFACE_CREATE — the content grows
            // with the frame instead of only the shadow.
            if id == WindowId::AppClient {
                self.push_app_content_rect();
            }
        }
    }

    /// Tell the app-client its current CONTENT rect (`OP_SURFACE_RECT`) so it
    /// re-creates its surface at that size (WM owns geometry).
    pub(crate) fn push_app_content_rect(&mut self) {
        // The content reserves `title_h` for the WM-drawn title bar. This is
        // INTENT-driven (`app_title_h`), NOT fullscreen-driven: a titled app that
        // maximizes keeps its title bar (□ = maximize), so the content stays
        // `frame − title`; only an intent-chromeless shell/single-app surface
        // gets the whole frame.
        let title = self.app_title_h();
        let cw = self.app_win.w.min(u32::from(u16::MAX)) as u16;
        let ch = self.app_win.h.saturating_sub(title).min(u32::from(u16::MAX)) as u16;
        let rect = nexus_display_proto::client_surface::encode_surface_rect(0, 0, cw, ch);
        let _ = self.send_app_frame(&rect);
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
    /// resize, re-render, damage the new region. The SINGLE geometry-apply
    /// path shared by edge resize, snap halves, and the fullscreen toggle.
    pub(super) fn apply_window_frame(&mut self, id: WindowId, x: i32, y: i32, w: u32, h: u32) {
        if id != WindowId::AppClient {
            // The desktop base is always the full display — never repositioned
            // or resized by the WM (its geometry follows the mode, pushed to
            // the shell app-host as the full content rect).
            return;
        }
        let old = self.window_damage_rect(id);
        self.queue_gpu_blit_rect(old);
        // Resize negotiation: let the frame grow to the requested size in BOTH
        // axes (mode-bounded). The render + glass composite are bounded to the
        // current band (`render_app_surface` / `glass_params`), so growing past
        // the band never over-reads; on release `end_window_resize` pushes this
        // size and the app re-creates its surface + band to match.
        let w = w.min(self.mode.width);
        let h = h.min(self.mode.height);
        self.app_win.set_frame(x, y, w, h);
        self.app_win.surface_dirty = true;
        // TASK #23: keep the title bar sharp at the TRUE frame width while the
        // band lags (live resize) — re-rasterized only on width change.
        self.update_app_title_overlay();
        let new = self.window_damage_rect(id);
        self.queue_gpu_blit_rect(new);
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
                // Only the floating app window has resizable borders; the
                // desktop base fills the display and never shows a resize
                // cursor (it would read as a broken affordance on the shell
                // and the greeter).
                if wid != WindowId::AppClient {
                    continue;
                }
                let frame = self.window_frame(wid);
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
    /// the stack's stable dock order). 2D-packed like the app window: the
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
            for (slot, _wid) in list[..n].iter().enumerate() {
                let cell = dock::dock_slot_rect(bar_local, slot);
                // Only the app-client window minimizes (the desktop base never
                // does); a per-app glyph lands with the DSL-shell dock (MOVE).
                let (icon, dim) =
                    (crate::assets::DOCK_SEARCH_ICON_BGRA, crate::assets::DOCK_SEARCH_ICON_DIM);
                let iy0 = cell.y + cell.height.saturating_sub(dim) / 2;
                if ly >= iy0 && ly < iy0 + dim {
                    let ix = cell.x + cell.width.saturating_sub(dim) / 2;
                    crate::assets::blend_icon_row(row, ix, icon, dim, ly - iy0, 255, glyph_tint);
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
