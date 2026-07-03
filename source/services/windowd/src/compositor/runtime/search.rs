// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: windowd compositor runtime — the Search window: open/close, scroll momentum, surface render, and damage rect.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: No tests (behavior covered via windowd QEMU smoke + host integration)
//!
//! Split out of `runtime/mod.rs` (TASK-0063 modularization). A child module of
//! `runtime`, so these `impl DisplayServerRuntime` methods read the runtime's
//! private fields directly; previously-private methods are widened to
//! `pub(super)` so the parent and sibling submodules can still call them.

use super::*;

impl DisplayServerRuntime {
    /// Show the Search window: acquire its atlas surfaces from the pool, refresh
    /// the filtered list, and damage its region. No-op if already open. If the
    /// atlas pool can't satisfy the request the window simply stays closed (the
    /// surfaces are released again), never a boot/handoff failure.
    pub(super) fn open_search(&mut self) {
        if self.shell_config.locked {
            return; // kiosk lockdown: no launcher windows
        }
        if !self.search.is_mounted() {
            // 2D-PACKED: content + blur are each window-width wide, so they pack
            // side-by-side in ONE band (instead of two full-width bands). The
            // content surface is CPU-rendered per-row at its column; gpud fills
            // the blur surface (it samples src_x natively). The blur cache is
            // best-effort — without one the window composites unblurred.
            let w = self.search.w;
            let h = self.search.h;
            let Some(content) = self.atlas_alloc.alloc(w, h) else {
                let _ = debug_println("windowd: search open — atlas pool full (content)");
                return;
            };
            let blur = self.atlas_alloc.alloc(w, h);
            if blur.is_none() {
                let _ = debug_println("windowd: search open — no blur cache (pool)");
            }
            self.search.mount(content, blur);
        }
        super::desktop_layer::search_filter(self.state.text_input(), &mut self.search_filtered);
        self.search.scroll = 0;
        self.search_scroll.set_offset(0.0);
        self.search_set_extent();
        self.search_scroll_last_ns = 0;
        self.search.visible = true;
        // Mirror into the z/focus stack: an opening window comes up on top.
        self.show_window(crate::window_scene::WindowId::Search);
        self.search.surface_dirty = true;
        self.queue_dirty_rect(self.search_window_rect());
    }

    /// Feed the shared momentum engine the Search list's scrollable extent (px):
    /// `viewport` = the visible list rows, `content` = all filtered rows. Called on
    /// open + whenever the filter changes the row count (re-clamps position).
    pub(super) fn search_set_extent(&mut self) {
        use super::desktop_layer::{search_visible_rows, SEARCH_LIST_ROW_H};
        let content = self.search_filtered.len() as u32 * SEARCH_LIST_ROW_H;
        let viewport = search_visible_rows(self.search.h) * SEARCH_LIST_ROW_H;
        self.search_scroll.set_extent(viewport as f32, content as f32);
    }

    /// Map the momentum engine's pixel offset to a whole-row list slice and, if the
    /// row changed, mark the Search surface dirty + damage its region. Returns true
    /// if the visible slice moved (so the pacer keeps ticking). The packed surface
    /// stays window-height (atlas-budget friendly — a full-list render-once surface
    /// would not fit beside chat's overscan), so scroll is a cheap re-render on the
    /// row boundary, driven by the SAME eased momentum as the chat window (E2).
    pub(super) fn commit_search_scroll_position(&mut self) -> bool {
        use super::desktop_layer::{search_max_scroll_for, search_visible_rows, SEARCH_LIST_ROW_H};
        let row = (self.search_scroll.offset_px().max(0) as u32 / SEARCH_LIST_ROW_H)
            .min(search_max_scroll_for(
                self.search_filtered.len(),
                search_visible_rows(self.search.h),
            ));
        if row == self.search.scroll {
            return false;
        }
        self.search.scroll = row;
        self.search.surface_dirty = true;
        self.queue_dirty_rect(self.search_window_rect());
        true
    }

    /// Advance the Search scroll momentum one frame (mirror of `tick_chat_scroll`):
    /// integrate the eased/coasting offset over real elapsed time, then commit the
    /// row slice. Returns true while still moving so the present pacer keeps ticking.
    pub(crate) fn tick_search_scroll(&mut self, now_ns: u64) -> bool {
        if !self.search.visible || !self.search_scroll.is_animating() {
            self.search_scroll_last_ns = 0;
            return false;
        }
        let dt_ns = if self.search_scroll_last_ns == 0 || now_ns <= self.search_scroll_last_ns {
            8_333_333
        } else {
            now_ns - self.search_scroll_last_ns
        };
        self.search_scroll_last_ns = now_ns;
        let still = self.search_scroll.tick(dt_ns);
        if !still {
            self.search_scroll_last_ns = 0;
        }
        self.commit_search_scroll_position();
        still
    }

    /// Hide the Search window: release its atlas surfaces back to the pool so the
    /// closed window costs zero atlas rows, and damage its (now vacated) region.
    pub(super) fn close_search(&mut self) {
        self.search.visible = false;
        // Mirror into the z/focus stack (focus falls to the next-top window).
        self.hide_window(crate::window_scene::WindowId::Search);
        self.search.end_drag();
        let rect = self.search_window_rect();
        if let Some((content, blur)) = self.search.unmount() {
            self.atlas_alloc.free(content);
            if let Some(blur) = blur {
                self.atlas_alloc.free(blur);
            }
        }
        self.queue_dirty_rect(rect);
    }

    /// Shell-P2b: render the Search window (title + close + filter field +
    /// filtered word list) into its atlas. Re-rendered when the filter text,
    /// close-hover, or visibility changes.
    pub(super) fn render_search_surface(&mut self) -> Result<(), WindowdError> {
        let Some(handle) = self.framebuffer else {
            return Ok(());
        };
        let stride = self.mode.stride as usize;
        if self.band_scratch.len() < stride * ROW_WRITE_CHUNK {
            return Err(WindowdError::BufferLengthMismatch);
        }
        let Some(surface) = self.search.atlas else {
            return Ok(()); // unmounted (hidden) — nothing to render
        };
        let abs_row = surface.abs_row;
        let col_off = surface.x as usize * 4; // packed column → byte offset into each row
        let h = self.search.h.min(surface.height);
        let w = self.search.w.min(surface.width);
        let row_bytes = w as usize * 4;
        let title_hover = self.search.title_hover;
        // Fullscreen renders square (the composite drops the radius too).
        let corner_radius = if self.windows.is_fullscreen(crate::window_scene::WindowId::Search) {
            0
        } else {
            super::desktop_layer::SEARCH_RADIUS
        };
        let scroll = self.search.scroll;
        let total = self.search_filtered.len();
        let visible_rows = super::desktop_layer::search_visible_rows(h);
        let visible_end = (scroll as usize + visible_rows as usize).min(total);
        let visible_start = (scroll as usize).min(visible_end);
        // Disjoint field borrows: filter text (state), filtered words, scratch band.
        let filter_text = self.state.text_input();
        let visible = &self.search_filtered[visible_start..visible_end];
        let band = &mut self.band_scratch;
        // The Search surface is 2D-PACKED (sub-stride, at column `surface.x`), so
        // its rows are NOT contiguous in the VMO — write per-row (the window's
        // `w*4` bytes at column `x`). Re-render only fires on open/filter/scroll,
        // so the per-row syscall count is fine (not a per-frame hot path).
        for ly in 0..h {
            let row = &mut band[0..stride];
            row[..row_bytes].fill(0);
            super::desktop_layer::draw_search_window_row(
                ly, row, w, visible_rows, filter_text, visible, scroll, total, title_hover,
                corner_radius,
            )?;
            let dst = (abs_row + ly) as usize * stride + col_off;
            vmo_write(handle, dst, &row[..row_bytes])
                .map_err(|_| WindowdError::BufferLengthMismatch)?;
        }
        Ok(())
    }

    /// Damage rect of the Search window (with a shadow-halo margin).
    pub(super) fn search_window_rect(&self) -> DamageRect {
        self.search.damage_rect(self.mode.width, self.mode.height)
    }
}
