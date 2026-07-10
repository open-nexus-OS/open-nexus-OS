// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! ⚠ CLEANUP-MAP (docs/dev/ui/windowd-cleanup-map.md): DELETE (LEGACY) — Settings wird eine DSL-App.
//! DO NOT EXTEND — new capability belongs at the target, not here.
//

//! CONTEXT: windowd compositor runtime — the Settings window (TASK-0072): a
//! third `ShellWindow` instance opened from the topbar Edit → Settings menu.
//! Static glass panel (no scroll) showing the Appearance section; the live
//! light/dark toggle + settingsd wiring land in Phase 10. Mirrors the Search
//! window's on-demand atlas lifecycle (acquire on show, release on hide).
//! OWNERS: @ui
//! STATUS: Experimental
//! API_STABILITY: Unstable

use super::*;

impl DisplayServerRuntime {
    /// Toggle the Settings window (Edit → Settings). Restores it if minimized,
    /// closes it if already open, else opens it — the same launcher semantics as
    /// Chat/Search so one menu entry cleanly toggles.
    pub(super) fn toggle_settings(&mut self) {
        if self.windows.is_minimized(crate::window_scene::WindowId::Settings) {
            self.restore_window(crate::window_scene::WindowId::Settings);
        } else if self.settings_win.visible {
            self.close_settings();
        } else {
            self.open_settings();
        }
    }

    /// Show the Settings window: acquire its atlas surface(s) from the on-demand
    /// pool, then damage its region. No-op if already open. If the pool can't
    /// back it the window stays closed (never a boot/handoff failure).
    pub(super) fn open_settings(&mut self) {
        if self.shell_config.locked {
            return; // kiosk lockdown: no launcher windows
        }
        if !self.settings_win.is_mounted() {
            let w = self.settings_win.w;
            let h = self.settings_win.h;
            let Some(content) = self.atlas_alloc.alloc(w, h) else {
                let _ = debug_println("windowd: settings open — atlas pool full (content)");
                return;
            };
            let blur = self.atlas_alloc.alloc(w, h);
            if blur.is_none() {
                let _ = debug_println("windowd: settings open — no blur cache (pool)");
            }
            self.settings_win.mount(content, blur);
        }
        self.settings_win.visible = true;
        self.show_window(crate::window_scene::WindowId::Settings);
        self.settings_win.surface_dirty = true;
        let _ = debug_println("windowd: settings window open");
        self.queue_dirty_rect(self.settings_window_rect());
    }

    /// Hide the Settings window: release its atlas surface(s) back to the pool so
    /// the closed window costs zero atlas rows, and damage its vacated region.
    pub(super) fn close_settings(&mut self) {
        self.settings_win.visible = false;
        self.hide_window(crate::window_scene::WindowId::Settings);
        self.settings_win.end_drag();
        let rect = self.settings_window_rect();
        if let Some((content, blur)) = self.settings_win.unmount() {
            self.atlas_alloc.free(content);
            if let Some(blur) = blur {
                self.atlas_alloc.free(blur);
            }
        }
        let _ = debug_println("windowd: settings window close");
        self.queue_dirty_rect(rect);
    }

    /// The current settings values shown in the panel: the live theme mode +
    /// the font family. The theme reflects `self.theme_mode` (toggled by
    /// clicking the Theme row, Phase 9); TODO(Phase 10): persist via settingsd.
    fn settings_values(&self) -> (&'static str, &'static str) {
        let theme = match self.theme_mode {
            crate::theme::ThemeMode::Dark => "Dark",
            crate::theme::ThemeMode::Light => "Light",
        };
        (theme, crate::assets::FONT_FAMILY)
    }

    /// Render the Settings window's static body into its atlas surface. Called
    /// when the surface is dirty (open / hover change), never per move.
    pub(super) fn render_settings_surface(&mut self) -> Result<(), WindowdError> {
        let Some(handle) = self.framebuffer else {
            return Ok(());
        };
        let Some(surface) = self.settings_win.atlas else {
            return Ok(()); // unmounted (hidden) — nothing to render
        };
        let stride = self.mode.stride as usize;
        if self.band_scratch.len() < stride * ROW_WRITE_CHUNK {
            return Err(WindowdError::BufferLengthMismatch);
        }
        let abs_row = surface.abs_row;
        let col_off = surface.x as usize * 4; // packed column → byte offset per row
        let h = self.settings_win.h.min(surface.height);
        let w = self.settings_win.w.min(surface.width);
        let row_bytes = w as usize * 4;
        let title_hover = self.settings_win.title_hover;
        let corner_radius = if self.windows.is_fullscreen(crate::window_scene::WindowId::Settings) {
            0
        } else {
            super::desktop_layer::SETTINGS_RADIUS
        };
        let (theme, font) = self.settings_values();
        let tk = self.theme(); // 'static token snapshot — no borrow conflict with band_scratch
        let band = &mut self.band_scratch;
        // 2D-PACKED surface (sub-stride at column `surface.x`): write per row.
        // Static panel → renders only on open/hover, not a per-frame hot path.
        for ly in 0..h {
            let row = &mut band[0..stride];
            row[..row_bytes].fill(0);
            super::desktop_layer::draw_settings_window_row(
                ly,
                row,
                w,
                theme,
                font,
                tk,
                title_hover,
                corner_radius,
            )?;
            let dst = (abs_row + ly) as usize * stride + col_off;
            vmo_write(handle, dst, &row[..row_bytes])
                .map_err(|_| WindowdError::BufferLengthMismatch)?;
        }
        Ok(())
    }

    /// Damage rect of the Settings window (with a shadow-halo margin).
    pub(super) fn settings_window_rect(&self) -> DamageRect {
        self.settings_win.damage_rect(self.mode.width, self.mode.height)
    }
}
