// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! BOUNDARY: this file is the FLOATING app-window surface rendering + geometry
//! slice of the app-surface service (a pure move out of `app_window.rs`, no
//! behavior change): acquire/release the atlas band, blit the surface rows out
//! of the app's VMO under the WM title bar (ADR-0042 damage-blit), the
//! live-resize title overlay, and the window's atlas-band geometry. It MUST NOT
//! grow window-chrome/sizing/decoration logic — that is the `ui/widgets/window`
//! widget's job (see `app_window.rs` header). Chrome + layout live in the widget
//! + scene graph; this only moves surface bytes into the band.
//! OWNERS: @ui @runtime
//! STATUS: Experimental (TASK-0080D R1)
//! API_STABILITY: Unstable
//! ADR: docs/adr/0042-cross-process-surface-transport.md

use super::app_window::{nexus_abi_cap_close, APP_WIN_RADIUS};
use super::*;

impl DisplayServerRuntime {
    /// Acquire atlas surfaces + show the window (mirrors `open_dsl_demo`).
    pub(super) fn open_app_window(&mut self, idx: usize) -> bool {
        if !self.apps[idx].win.is_mounted() {
            let w = self.apps[idx].win.w;
            let h = self.apps[idx].win.h;
            // WebRender compositor-scroll: the CONTENT band is physically TALL —
            // it holds the whole resident content ONCE (WM title + the app's
            // packed header/footer/content band) so a scroll is a pure gpud
            // `src_row` shift, no per-frame re-blit. A non-scroll surface
            // (`scroll_id == 0`) keeps the VISIBLE-sized band (unchanged).
            let content_rows =
                if self.apps[idx].scroll_id != 0 { self.app_band_height(idx).max(h) } else { h };
            let Some(content) = self.atlas_alloc.alloc(w, content_rows) else {
                let _ = debug_println(&alloc::format!(
                    "WINDOWD: surface open FAIL atlas (need={}x{} rows_remaining={})",
                    w,
                    content_rows,
                    self.atlas_alloc.rows_remaining()
                ));
                return false;
            };
            // The BLUR band stays VISIBLE-sized (the glass clamps to w×h anyway;
            // a tall blur band would starve the atlas). A desktop/full-screen
            // surface uses the R1 layer path (`BackdropCache::None`), so it needs
            // NO per-window blur band at all.
            let blur = if self.app_is_desktop_surface(idx) {
                None
            } else {
                self.atlas_alloc.alloc(w, h) // best-effort
            };
            self.apps[idx].win.mount(content, blur);
        }
        self.apps[idx].win.visible = true;
        self.show_window(crate::window_scene::WindowId::App(idx as u8));
        self.apps[idx].win.surface_dirty = true;
        self.apps[idx].surface_dirty_rows = None; // (re)shown: full re-blit
        let rect = self.app_window_rect(idx);
        self.queue_dirty_rect(rect);
        true
    }

    pub(super) fn close_app_window(&mut self, idx: usize) {
        let vacated = self.app_window_rect(idx);
        self.apps[idx].win.visible = false;
        self.update_app_title_overlay(idx); // frees (band drops below)
        self.hide_window(crate::window_scene::WindowId::App(idx as u8));
        self.apps[idx].win.end_drag();
        self.release_app_surface_band(idx);
        // The backdrop blur is destination-so-far: any window whose cached
        // blur was captured while the closed window overlapped it keeps a
        // GHOST of it under the glass. Invalidate every intersecting window's
        // blur cache (GPU re-blur next composite — cheap).
        self.invalidate_overlapping_blur(vacated, idx);
        // W4 slot recycling ("öffnen öffnen schließen … super effizient"): a
        // user-close ENDS the binding — destroy the surface registration
        // (frees the registry entry + the app's VMO capability), release the
        // event channel (cap + nonce table entry), and reset the slot so the
        // NEXT launch reuses it. Without this every close leaked the slot —
        // four opens+closes exhausted the window quota — and the surface/VMO
        // registry grew forever. The parked app process itself is the open
        // reaper (#29); it costs zero CPU (its frame pulses are withheld and
        // its presents hit the stale-id guard).
        if let Some(id) = self.apps[idx].surface_id.take() {
            if let Ok(vmo_slot) = self.client_surfaces.destroy(id) {
                let _ = nexus_abi_cap_close(vmo_slot);
            }
        }
        #[cfg(nexus_env = "os")]
        if let Some(ch) = self.apps[idx].event_channel.take() {
            if let Some(pos) =
                self.event_channels[..self.event_channels_len].iter().position(|e| e.1 == ch)
            {
                // Compact the nonce table (order-preserving shift).
                for i in pos..self.event_channels_len - 1 {
                    self.event_channels[i] = self.event_channels[i + 1];
                }
                self.event_channels_len -= 1;
            }
            let _ = nexus_abi_cap_close(ch);
        }
        self.apps[idx].frame_pulse_pending = false;
        self.apps[idx].surface_dirty_rows = None;
        self.apps[idx].scroll_id = 0;
        self.apps[idx].content_h = 0;
        self.apps[idx].header_h = 0;
        self.apps[idx].footer_h = 0;
        self.apps[idx].scroll_rows = 0;
        self.apps[idx].layer_count = 0;
        // Retire the transform override (it survives full presents now):
        // the slot's next tenant must not inherit a faded-out state.
        self.apps[idx].transform = WinTransform::IDENTITY;
        self.apps[idx].pending_wm = None;
        self.send_layer_transform(idx);
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

    /// Re-mount every z-visible, non-minimized app window that lost its atlas
    /// band to occlusion residency (fullscreen released it) — idempotent, and
    /// a no-op while a fullscreen window still covers the stack. Called on
    /// leave-fullscreen and after band releases (rows may have freed up).
    pub(super) fn ensure_visible_bands(&mut self) {
        if self.windows.fullscreen_active().is_some() {
            return; // still covered — occluded windows stay bandless
        }
        for idx in 0..self.apps.len() {
            let wid = crate::window_scene::WindowId::App(idx as u8);
            if self.apps[idx].surface_id.is_some()
                && self.apps[idx].win.visible
                && self.windows.is_visible(wid)
                && !self.apps[idx].win.is_mounted()
            {
                let _ = self.open_app_window(idx);
            }
        }
    }

    /// Free the app window's atlas band(s) WITHOUT touching window state
    /// (visibility, fullscreen, position). Used by the resize/fullscreen
    /// re-create (protocol SURFACE_DESTROY): the band must be reclaimed before
    /// the new surface allocates one, but the window keeps its geometry + mode so
    /// the re-created surface resumes in place. `close_app_window` hides first,
    /// then calls this.
    pub(super) fn release_app_surface_band(&mut self, idx: usize) {
        let rect = self.app_window_rect(idx);
        if let Some((content, blur)) = self.apps[idx].win.unmount() {
            self.atlas_alloc.free(content);
            if let Some(blur) = blur {
                self.atlas_alloc.free(blur);
            }
        }
        self.queue_dirty_rect(rect);
    }

    /// Blits the app surface out of its VMO into the window's atlas band:
    /// title bar drawn by windowd (server-side decoration), body rows read
    /// via `vmo_read` — the ADR-0042 damage-blit. Bounded by the surface
    /// dims validated at create.
    pub(super) fn render_app_surface(&mut self, idx: usize) -> Result<(), WindowdError> {
        let Some(handle) = self.framebuffer else {
            return Ok(());
        };
        let Some(surface) = self.apps[idx].win.atlas else {
            return Ok(());
        };
        // BY ID: `get()` (first live) is ambiguous once several surfaces coexist.
        let Some(client) =
            self.apps[idx].surface_id.and_then(|id| self.client_surfaces.get_by_id(id)).copied()
        else {
            return Ok(());
        };
        let stride = self.mode.stride as usize;
        if self.band_scratch.len() < stride {
            return Err(WindowdError::BufferLengthMismatch);
        }
        let abs_row = surface.abs_row;
        let col_off = surface.x as usize * 4;
        let win_w = self.apps[idx].win.w.min(surface.width);
        let win_h = self.apps[idx].win.h.min(surface.height);
        let body_w = (client.width as u32).min(win_w);
        let body_row_bytes = body_w as usize * 4;
        let src_stride = client.width as usize * 4;
        let title_hover = self.apps[idx].win.title_hover;
        // Corners: a full-screen presentation (declared desktop/fullscreen) and
        // the transient user-maximize are edge-to-edge (radius 0); floating
        // windows keep the rounded glass frame.
        let corner_radius = if self.app_presentation(idx).full_screen
            || self.windows.is_fullscreen(crate::window_scene::WindowId::App(idx as u8))
        {
            0
        } else {
            APP_WIN_RADIUS
        };
        let tk = self.theme();
        // Chromeless when `title_h == 0` (a `plain`/desktop-style surface, e.g.
        // the shell): the title-bar block never runs and the body fills from
        // row 0. A normal window keeps `APP_TITLE_H` — the WM-drawn frame.
        let title_h = self.apps[idx].win.title_h;
        // WebRender compositor-scroll: a scrollable surface blits the WHOLE tall
        // band ONCE (WM title + the app's packed header/footer/content band), so
        // a scroll is a pure gpud `src_row` shift with no re-blit. The client's
        // VMO is physically tall (`band_body_h` rows), independent of the frame's
        // VISIBLE `client.height`. A non-scroll surface is byte-identical to
        // before: iterate `win_h`, body bound = `client.height`.
        let scrollable = self.apps[idx].scroll_id != 0;
        let band_body_h = self.apps[idx]
            .header_h
            .saturating_add(self.apps[idx].footer_h)
            .saturating_add(self.apps[idx].content_h);
        let (blit_rows, body_limit) = if scrollable {
            (title_h.saturating_add(band_body_h).min(surface.height), band_body_h)
        } else {
            (win_h, client.height as u32)
        };
        // Damage-bounded blit (ADR-0042, the 120Hz damage contract): a
        // non-scroll present WITH damage rects re-copies only those body rows —
        // the title chrome and untouched body rows keep their band bytes (a
        // 16-row animation present costs 16 row-copies, not a full window +
        // chrome re-raster at animation rate).
        if title_h > 0 {
            // Rebuild the widget chrome raster only if (w/hover/theme/radius)
            // changed — bounded blits skip title rows entirely.
            self.ensure_chrome_cache(win_w, title_h, title_hover, corner_radius);
        }
        let (ly_start, ly_end) = if scrollable {
            (0, blit_rows)
        } else {
            match self.apps[idx].surface_dirty_rows {
                Some((y0, y1)) => (
                    title_h.saturating_add(y0).min(blit_rows),
                    title_h.saturating_add(y1).min(blit_rows),
                ),
                None => (0, blit_rows),
            }
        };
        for ly in ly_start..ly_end {
            let row_bytes = win_w as usize * 4;
            let row = &mut self.band_scratch[0..stride];
            row[..row_bytes].fill(0);
            if ly < title_h {
                // Chrome rows come from the WIDGET raster cache (P3.2
                // windows-as-widgets, `chrome_widget.rs`): the title bar is
                // built from `ui/widgets/window` chrome, laid out and painted
                // by nexus-scene-raster ONCE per chrome-state change — the
                // blit only memcpys. Never per-glyph work per present (the
                // chrome-cache pattern; `alloc-fail svc=gpud` history).
                let src = ly as usize * row_bytes;
                row[..row_bytes].copy_from_slice(&self.chrome_cache.buf[src..src + row_bytes]);
            } else {
                let body_y = ly - title_h;
                if body_y < body_limit {
                    // The damage-blit: one surface row out of the app's VMO.
                    #[cfg(nexus_env = "os")]
                    {
                        let src_off = body_y as usize * src_stride;
                        if nexus_abi::vmo_read(client.vmo_slot, src_off, &mut row[..body_row_bytes])
                            .is_err()
                        {
                            return Err(WindowdError::BufferLengthMismatch);
                        }
                    }
                } else {
                    // Below the app surface (max-size frame): glass tint.
                    // (`GLASS_TINT_ALPHA` relocated from the deleted legacy
                    // desktop_layer — ~59%, reads as frosted glass.)
                    const GLASS_TINT_ALPHA: u8 = 150;
                    crate::compositor::shell_window::write_tint_span(
                        row,
                        0,
                        win_w,
                        crate::theme::with_alpha(tk.glass_tint, GLASS_TINT_ALPHA),
                    );
                }
            }
            let dst = (abs_row + ly) as usize * stride + col_off;
            vmo_write(handle, dst, &self.band_scratch[..win_w as usize * 4])
                .map_err(|_| WindowdError::BufferLengthMismatch)?;
        }
        crate::atlas::note_atlas_content_write();
        Ok(())
    }

    /// Reconcile the live-resize title overlay (TASK #23) with the current
    /// frame: while the frame width differs from the content band (active
    /// resize / fullscreen transition), (re)rasterize the title bar at the
    /// TRUE frame width into a dedicated surface; once the re-created band
    /// catches up, free it. Bounded work: `title_h` rows × frame width, only
    /// on width CHANGES — the pretext discipline (cache chrome as a surface,
    /// never re-rasterize per present).
    pub(super) fn update_app_title_overlay(&mut self, idx: usize) {
        let title_h = self.apps[idx].win.title_h;
        let frame_w = self.apps[idx].win.w;
        let band_w = self.apps[idx].win.atlas.map(|a| a.width).unwrap_or(0);
        let needed = title_h > 0 && band_w > 0 && frame_w != band_w;
        if !needed {
            if let Some(s) = self.apps[idx].title_overlay.take() {
                self.atlas_alloc.free(s);
                self.apps[idx].title_overlay_w = 0;
                let rect = self.app_window_rect(idx);
                self.queue_dirty_rect(rect);
            }
            return;
        }
        if self.apps[idx].title_overlay_w == frame_w && self.apps[idx].title_overlay.is_some() {
            return; // already rendered at this width
        }
        if let Some(s) = self.apps[idx].title_overlay.take() {
            self.atlas_alloc.free(s);
        }
        let Some(surface) = self.atlas_alloc.alloc(frame_w, title_h) else {
            // Pool pressure: no overlay (the scaled band title shows instead —
            // degraded, never broken).
            self.apps[idx].title_overlay_w = 0;
            return;
        };
        if self.render_app_title_overlay(idx, surface, frame_w, title_h).is_err() {
            self.atlas_alloc.free(surface);
            self.apps[idx].title_overlay_w = 0;
            return;
        }
        self.apps[idx].title_overlay = Some(surface);
        self.apps[idx].title_overlay_w = frame_w;
        let rect = self.app_window_rect(idx);
        self.queue_dirty_rect(rect);
    }

    /// Rasterize the title bar at `w` into `surface` (the overlay path).
    fn render_app_title_overlay(
        &mut self,
        idx: usize,
        surface: crate::atlas::AtlasSurface,
        w: u32,
        title_h: u32,
    ) -> Result<(), WindowdError> {
        let Some(handle) = self.framebuffer else { return Ok(()) };
        let stride = self.mode.stride as usize;
        if self.band_scratch.len() < stride {
            return Err(WindowdError::BufferLengthMismatch);
        }
        let corner_radius = if self.app_presentation(idx).full_screen
            || self.windows.is_fullscreen(crate::window_scene::WindowId::App(idx as u8))
        {
            0
        } else {
            APP_WIN_RADIUS
        };
        let hover = self.apps[idx].win.title_hover;
        // P3.2: the overlay renders from the SAME widget chrome cache, at the
        // overlay width (re-rasters only on a width/hover/theme change).
        self.ensure_chrome_cache(w, title_h, hover, corner_radius);
        let row_bytes = w as usize * 4;
        for ly in 0..title_h.min(surface.height) {
            let row = &mut self.band_scratch[0..stride];
            let src = ly as usize * row_bytes;
            row[..row_bytes].copy_from_slice(&self.chrome_cache.buf[src..src + row_bytes]);
            let dst = (surface.abs_row + ly) as usize * stride + surface.x as usize * 4;
            vmo_write(handle, dst, &self.band_scratch[..w as usize * 4])
                .map_err(|_| WindowdError::BufferLengthMismatch)?;
        }
        crate::atlas::note_atlas_content_write();
        Ok(())
    }

    pub(super) fn app_window_rect(&self, idx: usize) -> DamageRect {
        self.apps[idx].win.damage_rect(self.mode.width, self.mode.height)
    }

    /// The full atlas-band height (rows) for a SCROLLABLE app surface: the WM
    /// title bar + the app's packed band (fixed header + fixed footer + the tall
    /// resident content, `header_h + footer_h + content_h`). This is what the
    /// tall content band is alloc'd to and what `render_app_surface` blits ONCE.
    /// Only meaningful when `scroll_id != 0`.
    pub(super) fn app_band_height(&self, idx: usize) -> u32 {
        self.apps[idx]
            .win
            .title_h
            .saturating_add(self.apps[idx].header_h)
            .saturating_add(self.apps[idx].footer_h)
            .saturating_add(self.apps[idx].content_h)
    }

    /// True when the app surface composes edge-to-edge without a cached-blur
    /// band. Declaratively: any full-screen presentation — PLUS the transient
    /// user-toggled fullscreen ("□"), which is WM state, not intent. This is an
    /// ATLAS-BUDGET decision (skip the blur band; a display-sized band would
    /// starve the atlas); chrome is decided by `app_title_h`.
    fn app_is_desktop_surface(&self, idx: usize) -> bool {
        self.app_presentation(idx).full_screen
            || self.windows.is_fullscreen(crate::window_scene::WindowId::App(idx as u8))
    }
}
