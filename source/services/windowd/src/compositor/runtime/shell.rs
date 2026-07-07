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
    /// Requests an app launch from the lifecycle broker (RFC-0065: SystemUI
    /// only REQUESTS; abilitymgr owns lifecycle + spawn). Wire:
    /// `[A,M,ver,OP_LAUNCH, app_len, app…, abil_len, abil…]`; the reply is
    /// drained bounded so the shared response queue never fills up.
    pub(super) fn launch_app(&mut self, app_id: &str) {
        let _ = debug_println(&alloc::format!("windowd: launch request app={app_id}"));
        #[cfg(nexus_env = "os")]
        {
            use nexus_ipc::Client as _;
            // Resolve the broker route lazily WITH retries and cache it:
            // one `new_for` = one ~100ms routing window ("caller-level
            // retries handle longer waits" — the query_route contract);
            // a single attempt failed live (user report 2026-07-07).
            if self.abilitymgr_client.is_none() {
                for _ in 0..20 {
                    if let Ok(resolved) = nexus_ipc::KernelClient::new_for("abilitymgr") {
                        self.abilitymgr_client = Some(resolved);
                        break;
                    }
                    let _ = nexus_abi::yield_();
                }
            }
            let Some(client) = self.abilitymgr_client.as_ref() else {
                let _ = debug_println("windowd: FAIL launch route (abilitymgr)");
                return;
            };
            let app = app_id.as_bytes();
            const ABIL: &[u8] = b"main";
            let mut req = alloc::vec::Vec::with_capacity(6 + app.len() + ABIL.len());
            req.extend_from_slice(&[b'A', b'M', 1, 1]); // MAGIC, ver, OP_LAUNCH
            req.push(app.len() as u8);
            req.extend_from_slice(app);
            req.push(ABIL.len() as u8);
            req.extend_from_slice(ABIL);
            if client.send(&req, nexus_ipc::Wait::NonBlocking).is_err() {
                let _ = debug_println("windowd: FAIL launch send");
                return;
            }
            // Drain the reply bounded (status logging only — the launch
            // outcome is abilitymgr's marker chain).
            let mut rsp = [0u8; 16];
            for _ in 0..2_000 {
                match client.recv_into(nexus_ipc::Wait::NonBlocking, &mut rsp) {
                    Ok(n) if n >= 5 && rsp[3] == 0x81 => {
                        if rsp[4] != 0 {
                            let _ = debug_println("windowd: launch denied");
                        }
                        return;
                    }
                    Ok(_) => {}
                    Err(_) => {
                        let _ = nexus_abi::yield_();
                    }
                }
            }
        }
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

    /// Toggle light↔dark from the settings panel (TASK-0072 Phase 10): switch the
    /// live UI immediately, then persist the new mode via settingsd so it survives
    /// a reboot. The live switch already happened, so a transport failure only
    /// misses persistence — never blocks the UI.
    pub(super) fn toggle_theme(&mut self) {
        let next = self.theme_mode.toggled();
        self.set_theme_mode(next);
        #[cfg(all(nexus_env = "os", target_os = "none"))]
        {
            if crate::settings_client::set_theme_mode(next) {
                let _ = debug_println("windowd: theme persist ok");
            } else {
                let _ = debug_println("windowd: theme persist unroutable");
            }
            // The user made a definite choice — a late boot probe must not revert it.
            self.mark_theme_user_set();
        }
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

/// Persisted-theme probe state (TASK-0072 Phase 10). Mirrors `SessionProbe`:
/// a one-shot-until-success GET of `ui.theme.mode` from settingsd, bounded so a
/// missing/slow settingsd degrades to the build-time default (Dark) instead of
/// retrying forever.
#[derive(Default)]
pub(super) struct ThemeProbe {
    /// The theme has been resolved (applied from settingsd, or the bound hit).
    done: bool,
    /// Attempts so far.
    attempts: u32,
    /// Monotonic deadline before the next attempt.
    next_try_ns: u64,
}

/// Probe cadence: settingsd is a background service, so on a display-first boot
/// the first attempts may race its bind. 250ms × 24 = ~6s before the default.
#[cfg(all(nexus_env = "os", target_os = "none"))]
const THEME_PROBE_INTERVAL_NS: u64 = 250_000_000;
#[cfg(all(nexus_env = "os", target_os = "none"))]
const THEME_PROBE_MAX_ATTEMPTS: u32 = 24;

#[cfg(all(nexus_env = "os", target_os = "none"))]
impl DisplayServerRuntime {
    /// One synchronous GET at the first-frame handoff: restores the persisted
    /// theme before the first present when settingsd is already up. A miss falls
    /// back to the cadenced probe below.
    pub(crate) fn theme_probe_at_handoff(&mut self) {
        if self.theme_probe.done {
            return;
        }
        self.theme_probe.attempts = self.theme_probe.attempts.saturating_add(1);
        self.try_restore_persisted_theme();
    }

    /// One probe step from the main loop. Returns `true` while the probe still
    /// needs pacing wakes (else the loop stays fully blocking).
    pub(crate) fn theme_probe_tick(&mut self, now_ns: u64) -> bool {
        if self.theme_probe.done {
            return false;
        }
        if self.is_handoff_pending() {
            return true;
        }
        if now_ns < self.theme_probe.next_try_ns {
            return true;
        }
        self.theme_probe.next_try_ns = now_ns.saturating_add(THEME_PROBE_INTERVAL_NS);
        self.theme_probe.attempts = self.theme_probe.attempts.saturating_add(1);
        self.try_restore_persisted_theme();
        if !self.theme_probe.done && self.theme_probe.attempts >= THEME_PROBE_MAX_ATTEMPTS {
            self.theme_probe.done = true; // settingsd unreachable → keep the default
            let _ = debug_println("windowd: theme default (settingsd unavailable)");
        }
        !self.theme_probe.done
    }

    /// GET `ui.theme.mode`; on success apply it (a no-op when it already matches)
    /// and mark the probe resolved. Emits an honest marker so the reboot-survival
    /// chain is observable end-to-end.
    fn try_restore_persisted_theme(&mut self) {
        if let Some(mode) = crate::settings_client::get_theme_mode() {
            self.theme_probe.done = true;
            self.set_theme_mode(mode);
            let _ = debug_println(&alloc::format!(
                "windowd: theme restored (mode={})",
                mode.as_str()
            ));
        }
    }

    /// The user set the theme via the settings panel: the probe must not later
    /// revert it with a stale GET (the persist + live switch already happened).
    pub(super) fn mark_theme_user_set(&mut self) {
        self.theme_probe.done = true;
    }
}
