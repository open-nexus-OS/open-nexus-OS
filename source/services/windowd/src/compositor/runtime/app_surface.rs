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

use super::*;
use super::app_window::{APP_CLOSE_W, APP_WIN_RADIUS};

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
            let content_rows = if self.apps[idx].scroll_id != 0 {
                self.app_band_height(idx).max(h)
            } else {
                h
            };
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
        let rect = self.app_window_rect(idx);
        self.queue_dirty_rect(rect);
        true
    }

    pub(super) fn close_app_window(&mut self, idx: usize) {
        self.apps[idx].win.visible = false;
        self.update_app_title_overlay(idx); // frees (band drops below)
        self.hide_window(crate::window_scene::WindowId::App(idx as u8));
        self.apps[idx].win.end_drag();
        self.release_app_surface_band(idx);
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
        let Some(client) = self.apps[idx]
            .surface_id
            .and_then(|id| self.client_surfaces.get_by_id(id))
            .copied()
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
            || self
                .windows
                .is_fullscreen(crate::window_scene::WindowId::App(idx as u8))
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
            (
                title_h.saturating_add(band_body_h).min(surface.height),
                band_body_h,
            )
        } else {
            (win_h, client.height as u32)
        };
        for ly in 0..blit_rows {
            let row_bytes = win_w as usize * 4;
            let row = &mut self.band_scratch[0..stride];
            row[..row_bytes].fill(0);
            if ly < title_h {
                // Chrome (bar + title text + real icon controls `[– □ ×]` +
                // hover) is RASTERIZED into the band here, only when the band is
                // dirty (create/resize/hover/theme) — NOT per frame. It then
                // composites as ONE cached surface (`composite_glass`). This is
                // the glyph/chrome-CACHE pattern: never emit per-glyph vector
                // tiles every present (that floods gpud's non-freeing heap →
                // `alloc-fail svc=gpud`, the resize crash). The scene graph
                // renders this as a Surface (blit), not vector text.
                crate::compositor::shell_window::draw_title_bar_row(
                    ly,
                    row,
                    win_w,
                    "App",
                    title_h,
                    APP_CLOSE_W,
                    title_hover,
                    corner_radius,
                    tk,
                )?;
            } else {
                let body_y = ly - title_h;
                if body_y < body_limit {
                    // The damage-blit: one surface row out of the app's VMO.
                    #[cfg(nexus_env = "os")]
                    {
                        let src_off = body_y as usize * src_stride;
                        if nexus_abi::vmo_read(
                            client.vmo_slot,
                            src_off,
                            &mut row[..body_row_bytes],
                        )
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
            || self
                .windows
                .is_fullscreen(crate::window_scene::WindowId::App(idx as u8))
        {
            0
        } else {
            APP_WIN_RADIUS
        };
        let hover = self.apps[idx].win.title_hover;
        let tk = self.theme();
        for ly in 0..title_h.min(surface.height) {
            let row = &mut self.band_scratch[0..stride];
            row[..w as usize * 4].fill(0);
            crate::compositor::shell_window::draw_title_bar_row(
                ly,
                row,
                w,
                "App",
                title_h,
                APP_CLOSE_W,
                hover,
                corner_radius,
                tk,
            )?;
            let dst = (surface.abs_row + ly) as usize * stride + surface.x as usize * 4;
            vmo_write(handle, dst, &self.band_scratch[..w as usize * 4])
                .map_err(|_| WindowdError::BufferLengthMismatch)?;
        }
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
            || self
                .windows
                .is_fullscreen(crate::window_scene::WindowId::App(idx as u8))
    }
}
