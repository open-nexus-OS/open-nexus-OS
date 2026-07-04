// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: windowd compositor runtime — the shell chrome — runtime shell-config apply, Apps menu/launcher, and topbar/sidepanel/dropdown surfaces.
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
    /// Apply a new shell configuration at runtime (the SystemUI-driven shell
    /// switch). Updates `shell_config`, re-renders + re-composites the chrome
    /// (topbar / side panel appear or disappear with `desktop_chrome`), closes the
    /// Apps dropdown (its topbar may vanish), and damages the chrome regions so
    /// both the virgl (rebuild-every-present) and mmio (damage-driven) paths show
    /// the new shell. Emits a marker for boot verification.
    pub(super) fn apply_shell_config(&mut self, cfg: systemui::ShellConfig) {
        let _ = debug_println(&alloc::format!(
            "windowd: shell switch product={} shell={} kind={} chrome={} locked={}",
            cfg.product_id, cfg.shell_id, cfg.shell_kind, cfg.desktop_chrome, cfg.locked,
        ));
        self.shell_config = cfg;
        self.open_topbar_menu = None;
        self.shell_surface_dirty = true;
        self.sidepanel_surface_dirty = true;
        self.dropdown_surface_dirty = true;
        // Kiosk lockdown: a locked shell suppresses the launcher windows entirely
        // (the "company configures a complete kiosk" policy) — close any open chat /
        // search so only the locked surface remains. `toggle_chat`/`open_search`
        // also refuse to open while locked.
        if self.shell_config.locked {
            if self.chat.visible {
                self.chat.visible = false;
                self.on_chat_window_closed();
            }
            if self.search.visible {
                self.close_search();
            }
        }
        use crate::compositor::desktop_layer::{
            SIDEPANEL_MARGIN, SIDEPANEL_W, TOPBAR_H, TOPBAR_TOP,
        };
        let w = self.mode.width;
        let h = self.mode.height;
        // Top strip (topbar) + right strip (side panel) — generous enough to cover
        // the chrome + its shadow halo so a removed chrome leaves no trail.
        self.queue_gpu_blit_rect(DamageRect {
            x: 0,
            y: 0,
            width: w,
            height: (TOPBAR_TOP + TOPBAR_H + 24).min(h),
        });
        let panel_w = (SIDEPANEL_W + SIDEPANEL_MARGIN + 48).min(w);
        self.queue_gpu_blit_rect(DamageRect {
            x: w.saturating_sub(panel_w),
            y: 0,
            width: panel_w,
            height: h,
        });
    }

    /// One-shot lazy fetch of the Apps menu from the bundle registry
    /// (`bundlemgrd` OP_LIST_APPS). Runs at most once (first dropdown open); on
    /// success it replaces the seed menu, resizes the open height, and re-renders
    /// the dropdown surface. On any failure the seed (Chat/Search) persists so the
    /// menu never regresses. Emits `windowd: apps ok (n=N)` / `windowd: apps seed`.
    pub(super) fn ensure_app_menu(&mut self) {
        if self.app_menu_fetched {
            return;
        }
        self.app_menu_fetched = true;
        match crate::registry_client::fetch_app_menu() {
            Some(menu) => {
                let n = menu.len();
                self.app_menu = menu;
                self.dropdown_h = self.app_menu.dropdown_full_h();
                self.dropdown_hover = None;
                self.dropdown_surface_dirty = true;
                let _ = debug_println(&alloc::format!("windowd: apps ok (n={n})"));
            }
            None => {
                let _ = debug_println("windowd: apps seed (registry unreachable)");
            }
        }
    }

    /// Launch an installed app that windowd does not host directly (e.g. a real
    /// `.nxb` bundle app). The ability lifecycle broker (`abilitymgr`) owns spawn;
    /// windowd only requests it. For now this records the intent via a marker — the
    /// abilitymgr launch handoff lands with the per-app-surface compositor path
    /// (TASK-0065 P4b). Named app so the chain is observable end-to-end.
    pub(super) fn launch_app(&mut self, app_id: &str) {
        let _ = debug_println(&alloc::format!("windowd: launch request app={app_id}"));
    }

    /// Cycle to the next registered product's shell (desktop → tablet → kiosk → …)
    /// via SystemUI's resolver, and apply it. The shell switcher's one action.
    pub(super) fn cycle_shell(&mut self) {
        let next = systemui::shell_config_next(&self.shell_config.product_id);
        self.apply_shell_config(next);
    }

    /// Shell-P2b: render the glass topbar into its atlas surface (rows
    /// `shell_atlas.abs_row..`, bar-local coords). Called when dirty (init /
    /// hover change). Each row is cleared first; the composite applies the
    /// rounded mask + backdrop blur + shadow.
    pub(super) fn render_shell_surface(&mut self) -> Result<(), WindowdError> {
        let Some(handle) = self.framebuffer else {
            return Ok(());
        };
        let stride = self.mode.stride as usize;
        if self.band_scratch.len() < stride * ROW_WRITE_CHUNK {
            return Err(WindowdError::BufferLengthMismatch);
        }
        let abs_row = self.shell_atlas.abs_row;
        let shell_h = self.shell_h;
        let bar_w = self.shell_w;
        let hover = self.topbar_hover;
        let menu_hover = self.topbar_menu_hover;
        let tk = self.theme();
        let band = &mut self.band_scratch;
        let mut band_start = 0u32;
        while band_start < shell_h {
            let band_end = (band_start + ROW_WRITE_CHUNK as u32).min(shell_h);
            let band_rows = (band_end - band_start) as usize;
            for (i, ly) in (band_start..band_end).enumerate() {
                let row = &mut band[i * stride..(i + 1) * stride];
                row.fill(0);
                super::desktop_layer::draw_topbar_row(ly, row, bar_w, hover, menu_hover, tk)?;
            }
            let dst = (abs_row + band_start) as usize * stride;
            vmo_write(handle, dst, &band[..band_rows * stride])
                .map_err(|_| WindowdError::BufferLengthMismatch)?;
            band_start = band_end;
        }
        Ok(())
    }

    /// Shell-P2b: render the glass side panel into its atlas surface (rows
    /// `sidepanel_atlas.abs_row..`, panel-local coords). Rendered once; the
    /// composite slides it in from the right and applies blur/rounding/shadow.
    pub(super) fn render_sidepanel_surface(&mut self) -> Result<(), WindowdError> {
        let Some(handle) = self.framebuffer else {
            return Ok(());
        };
        let stride = self.mode.stride as usize;
        if self.band_scratch.len() < stride * ROW_WRITE_CHUNK {
            return Err(WindowdError::BufferLengthMismatch);
        }
        let abs_row = self.sidepanel_atlas.abs_row;
        let panel_h = self.sidepanel_h;
        let panel_w = super::desktop_layer::SIDEPANEL_W;
        let tk = self.theme();
        let band = &mut self.band_scratch;
        let mut band_start = 0u32;
        while band_start < panel_h {
            let band_end = (band_start + ROW_WRITE_CHUNK as u32).min(panel_h);
            let band_rows = (band_end - band_start) as usize;
            for (i, ly) in (band_start..band_end).enumerate() {
                let row = &mut band[i * stride..(i + 1) * stride];
                row.fill(0);
                super::desktop_layer::draw_sidepanel_row(ly, row, panel_w, tk)?;
            }
            let dst = (abs_row + band_start) as usize * stride;
            vmo_write(handle, dst, &band[..band_rows * stride])
                .map_err(|_| WindowdError::BufferLengthMismatch)?;
            band_start = band_end;
        }
        Ok(())
    }

    /// The active theme's baked color snapshot (TASK-0072 Phase 9).
    pub(super) fn theme(&self) -> &'static crate::theme::ThemeTokens {
        match self.theme_mode {
            crate::theme::ThemeMode::Dark => &crate::assets::THEME_DARK,
            crate::theme::ThemeMode::Light => &crate::assets::THEME_LIGHT,
        }
    }

    /// Switch the active light/dark theme: swap the token snapshot, re-render
    /// every themed surface (invalidating glass blur caches), full-frame damage,
    /// and emit the honest marker. No-op when already in `mode`. Wired to
    /// settingsd's `ui.theme.mode` apply hook + the settings panel toggle
    /// (Phase 10); callable now for the boot-time application.
    pub(super) fn set_theme_mode(&mut self, mode: crate::theme::ThemeMode) {
        if self.theme_mode == mode {
            return;
        }
        self.theme_mode = mode;
        self.shell_surface_dirty = true;
        self.sidepanel_surface_dirty = true;
        self.dropdown_surface_dirty = true;
        self.chat.surface_dirty = true;
        self.chat.blur_valid = false;
        self.search.surface_dirty = true;
        self.search.blur_valid = false;
        self.settings_win.surface_dirty = true;
        self.settings_win.blur_valid = false;
        self.dock_dirty = true;
        self.queue_full_frame_damage();
        let _ = debug_println(&alloc::format!("uitheme: switched (to={})", mode.as_str()));
    }

    /// The topbar item whose dropdown is open (Apps=0 default when none, for
    /// stable geometry while the close animation runs).
    pub(super) fn dropdown_item(&self) -> usize {
        self.open_topbar_menu.unwrap_or(0)
    }

    /// The menu content for the currently open dropdown: the dynamic Apps menu
    /// (item 0) or the static Edit menu (item 2). One surface, one renderer.
    pub(super) fn active_menu(&self) -> &crate::app_menu::AppMenu {
        match self.open_topbar_menu {
            Some(2) => &self.edit_menu,
            _ => &self.app_menu,
        }
    }

    /// Shell-P2b: render the open topbar dropdown into its atlas (menu rows +
    /// hover). Composited below the open item with an animated reveal.
    pub(super) fn render_dropdown_surface(&mut self) -> Result<(), WindowdError> {
        let Some(handle) = self.framebuffer else {
            return Ok(());
        };
        let stride = self.mode.stride as usize;
        if self.band_scratch.len() < stride * ROW_WRITE_CHUNK {
            return Err(WindowdError::BufferLengthMismatch);
        }
        let abs_row = self.dropdown_atlas.abs_row;
        let h = self.dropdown_h;
        let w = super::desktop_layer::DROPDOWN_W;
        let hover = self.dropdown_hover;
        let menu = self.active_menu().clone();
        let tk = self.theme();
        let band = &mut self.band_scratch;
        let mut band_start = 0u32;
        while band_start < h {
            let band_end = (band_start + ROW_WRITE_CHUNK as u32).min(h);
            let band_rows = (band_end - band_start) as usize;
            for (i, ly) in (band_start..band_end).enumerate() {
                let row = &mut band[i * stride..(i + 1) * stride];
                row.fill(0);
                super::desktop_layer::draw_dropdown_row(&menu, ly, row, w, hover, tk)?;
            }
            let dst = (abs_row + band_start) as usize * stride;
            vmo_write(handle, dst, &band[..band_rows * stride])
                .map_err(|_| WindowdError::BufferLengthMismatch)?;
            band_start = band_end;
        }
        Ok(())
    }
}
