// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: windowd compositor runtime — shell POLICY plumbing: runtime
//! shell-config apply, app-launch requests to abilitymgr, theme apply/persist.
//! The legacy chrome renderers (topbar/sidepanel/dropdown/Apps menu) are
//! DELETED per the cleanup map — that UI is the DSL shell app-host's.
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
        // The chrome IS the DSL shell app-host's surface now — a shell switch
        // just changes policy (chrome flag, lockdown) and repaints everything.
        // Push the new profile so every app-host re-mounts with the matching
        // `ui/platform/<profile>/` override arms (tablet ⇄ desktop).
        self.push_app_profile();
        self.queue_full_frame_damage();
    }

    /// `OP_SURFACE_CONTROL` from a shell surface: windowd is the single
    /// presentation authority — apply the change LIVE, then persist via
    /// settingsd (the native-toggle path; a DSL Control Center can never
    /// desynchronize the compositor). Unknown controls are reported, not
    /// silently dropped.
    /// RECORDED FOLLOW-UP: enforce the sender ROLE (desktop-surface owner or
    /// settings-role app) once the server endpoint carries per-sender identity
    /// (the execd requester-id pattern) — today any surface client could send
    /// this; presentation-only blast radius, but it should be fail-closed.
    pub(crate) fn handle_surface_control(&mut self, frame: &[u8], _sender_sid: u64) {
        use nexus_display_proto::client_surface as wire;
        let Some((control, value)) = wire::decode_surface_control(frame) else {
            let _ = debug_println("WINDOWD: FAIL control (malformed)");
            return;
        };
        match control {
            wire::CONTROL_THEME => {
                let mode = if value == wire::THEME_LIGHT {
                    crate::theme::ThemeMode::Light
                } else {
                    crate::theme::ThemeMode::Dark
                };
                self.set_theme_mode(mode);
                #[cfg(all(nexus_env = "os", target_os = "none"))]
                {
                    let _ = crate::settings_client::set_theme_mode(mode);
                    self.mark_theme_user_set();
                }
            }
            wire::CONTROL_SHELL_PROFILE => {
                self.set_shell_profile_wire(value, true);
            }
            wire::CONTROL_THEME_ACCENT => {
                // Accent-palette pick: store + re-push the packed theme byte
                // to every app-host (they re-mount with the accent override),
                // then persist. windowd's own chrome has no accent-role
                // pixels today, so no local repaint beyond the push.
                if self.theme_accent != value {
                    self.theme_accent = value;
                    self.push_app_theme();
                    let _ = debug_println(&alloc::format!(
                        "uitheme: accent switched (to={value})"
                    ));
                    #[cfg(all(nexus_env = "os", target_os = "none"))]
                    {
                        let _ = crate::settings_client::set_theme_accent(value);
                    }
                }
            }
            wire::CONTROL_LAUNCH_PENDING => {
                // Shell-initiated app launch (svc.ability.launch): show the
                // wait ring until the fresh window's surface arrives.
                self.begin_cursor_wait();
            }
            // App-chrome window controls (window-kit app menu). The recv
            // path carries no sender identity (sid=0 observed), so the value
            // byte names the caller's own surface id: minimize/close =
            // `id`; mode = `id << 4 | WIN_MODE_*`. Fail-closed on no match.
            // RECORDED FOLLOW-UP: enforce per-sender identity once the
            // kernel meta carries it (presentation-only blast radius).
            wire::CONTROL_WIN_MINIMIZE => {
                if let Some(idx) = self.app_idx_by_surface(u32::from(value)) {
                    self.start_minimize_transition(idx);
                } else {
                    let _ = debug_println("WINDOWD: control win (no window for id)");
                }
            }
            wire::CONTROL_WIN_CLOSE => {
                if let Some(idx) = self.app_idx_by_surface(u32::from(value)) {
                    self.start_close_transition(idx);
                } else {
                    let _ = debug_println("WINDOWD: control win (no window for id)");
                }
            }
            wire::CONTROL_WIN_MODE => {
                let (sid, mode) = (u32::from(value >> 4), value & 0x0F);
                if let Some(idx) = self.app_idx_by_surface(sid) {
                    self.apply_window_mode(idx, mode);
                } else {
                    let _ = debug_println("WINDOWD: control win (no window for id)");
                }
            }
            other => {
                let _ = debug_println(&alloc::format!(
                    "WINDOWD: control unknown kind={other} value={value}"
                ));
            }
        }
    }

    /// Switch the shell to the product matching a `PROFILE_*` wire tag
    /// (tablet ⇄ desktop), optionally persisting `ui.shell.mode`. No-op when
    /// the profile already matches (idempotent — the boot restore path).
    pub(crate) fn set_shell_profile_wire(&mut self, profile: u8, persist: bool) {
        use nexus_display_proto::client_surface as wire;
        let (product, mode_str) = if profile == wire::PROFILE_TABLET {
            ("tablet", "tablet")
        } else {
            // The `default` product carries the desktop profile/shell.
            ("default", "desktop")
        };
        if self.shell_profile_wire() == profile {
            return;
        }
        let cfg = systemui::shell_config_for(product);
        self.apply_shell_config(cfg);
        #[cfg(all(nexus_env = "os", target_os = "none"))]
        if persist {
            if crate::settings_client::set_shell_mode(mode_str) {
                let _ = debug_println("windowd: shell mode persist ok");
            } else {
                let _ = debug_println("windowd: shell mode persist unroutable");
            }
        }
        #[cfg(not(all(nexus_env = "os", target_os = "none")))]
        let _ = (persist, mode_str);
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
        // Animated wait cursor until the fresh window's surface arrives
        // (`handle_surface_create` ends it; a failsafe deadline backs it up).
        self.begin_cursor_wait();
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
        // Theme-matched wallpaper: swap the baked source (same decoded size,
        // LUTs stay valid); the full-frame damage below repaints the display
        // AND the retained plane from the new pixels.
        #[cfg(nexus_env = "os")]
        if systemui::wallpaper_source_is_jpeg() {
            let (data, rows) =
                systemui::wallpaper_rle_for(mode == crate::theme::ThemeMode::Dark);
            self.source_frame.pixels = data;
            self.source_frame.rows = Some(rows);
            // Plane 0 (the boot-written wallpaper SOURCE plane) is only
            // written once at boot — rewrite it from the swapped source, or
            // every consumer sampling plane 0 keeps the old theme's pixels.
            let _ = self.write_source_frame_to_vmo();
            // gpud's wallpaper GL texture is a one-shot reveal upload from
            // plane 0 — tell it the plane changed so the next present
            // re-uploads (fire-and-forget; the present that follows the
            // full-frame damage below picks it up in order).
            if self.ensure_gpud_client() {
                if let Some(client) = self.gpud_client.as_ref() {
                    let _ = client.send(
                        &[nexus_display_proto::OP_WALLPAPER_DIRTY],
                        Wait::NonBlocking,
                    );
                }
            }
            // Full CPU repaint: mark every tile dirty (the GPU blit rect from
            // `queue_full_frame_damage` below alone skips the wallpaper bands).
            self.queue_dirty_rect(DamageRect {
                x: 0,
                y: 0,
                width: self.mode.width,
                height: self.mode.height,
            });
            // Fold-immune proof the swap ran (this decided a user-visible bug).
            let _ = nexus_abi::debug_write(match mode {
                crate::theme::ThemeMode::Dark => b"windowd: wallpaper swapped dark\n".as_slice(),
                crate::theme::ThemeMode::Light => b"windowd: wallpaper swapped light\n".as_slice(),
            });
        }
        // Live re-theme: tell the app-client so it re-renders in the new mode.
        self.push_app_theme();
        // The app-client window chrome follows the theme (re-rendered from
        // `self.theme()`); app/shell CONTENT re-themes via the pushed
        // OP_SURFACE_THEME (the DSL apps remount in the new mode).
        for slot in self.apps.iter_mut() {
            slot.win.surface_dirty = true;
            slot.surface_dirty_rows = None; // re-theme: full re-blit (chrome too)
            slot.win.blur_valid = false;
        }
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
            // settingsd is reachable — restore the persisted accent in the
            // same breath (apply-only): pushes the packed theme byte if it
            // differs from the boot default.
            if let Some(accent) = crate::settings_client::get_theme_accent() {
                if accent != self.theme_accent {
                    self.theme_accent = accent;
                    self.push_app_theme();
                    let _ = debug_println(&alloc::format!(
                        "windowd: theme accent restored (idx={accent})"
                    ));
                }
            }
            // …and the persisted shell mode (no second probe): apply-only,
            // never re-persist.
            if let Some(shell_mode) = crate::settings_client::get_shell_mode() {
                use nexus_display_proto::client_surface as wire;
                let profile = if shell_mode == "desktop" {
                    wire::PROFILE_DESKTOP
                } else {
                    wire::PROFILE_TABLET
                };
                self.set_shell_profile_wire(profile, false);
                let _ = debug_println(&alloc::format!(
                    "windowd: shell mode restored (mode={shell_mode})"
                ));
            }
        }
    }

    /// The user set the theme via the settings panel: the probe must not later
    /// revert it with a stale GET (the persist + live switch already happened).
    pub(super) fn mark_theme_user_set(&mut self) {
        self.theme_probe.done = true;
    }
}
