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
            cfg.product_id,
            cfg.shell_id,
            cfg.shell_kind,
            cfg.desktop_chrome,
            cfg.locked,
        ));
        self.shell_config = cfg;
        // The chrome IS the DSL shell app-host's surface now — a shell switch
        // just changes policy (chrome flag, lockdown) and repaints everything.
        // Apps re-select their `ui/platform/<profile>/` arms via the RFC-0083
        // snapshot (reemit, no remount).
        self.presentation_changed();
        self.queue_full_frame_damage();
    }

    /// `OP_SURFACE_CONTROL` from a shell surface: windowd is the single
    /// presentation authority — apply the change LIVE, then persist via
    /// settingsd (the native-toggle path; a DSL Control Center can never
    /// desynchronize the compositor). Unknown controls are reported, not
    /// silently dropped.
    ///
    /// RFC-0086 closes the old follow-up: every `CONTROL_WIN_*` verb is now
    /// gated on the kernel sender id matching the window's captured
    /// `owner_sid` (execd names app-hosts `app:<id>` via exec_v2). The value
    /// byte still RESOLVES the caller's surface, but it no longer carries
    /// authority — a spoofed id fails the gate (`control_gate`, host-tested).
    pub(crate) fn handle_surface_control(&mut self, frame: &[u8], sender_sid: u64) {
        use nexus_display_proto::client_surface as wire;
        let Some((control, value)) = wire::decode_surface_control(frame) else {
            let _ = debug_println("WINDOWD: FAIL control (malformed)");
            return;
        };
        match control {
            // RFC-0083 P5: CONTROL_THEME / CONTROL_THEME_ACCENT /
            // CONTROL_SHELL_PROFILE are RETIRED — settings writes go to
            // settingsd (the one authority) and arrive back as watch events.
            // Their numbers stay reserved in the wire crate; an old sender is
            // answered like any unknown control (reported, not silent).
            wire::CONTROL_LAUNCH_PENDING => {
                // Shell-initiated app launch (svc.ability.launch): show the
                // wait ring until the fresh window's surface arrives.
                self.begin_cursor_wait();
            }
            // App-chrome window controls (window-kit app menu): the value
            // byte names the caller's own surface id (minimize/close = `id`;
            // mode = `id << 4 | WIN_MODE_*`), the SID GATE enforces it is
            // really the caller's. Fail-closed on no match or a foreign sid.
            wire::CONTROL_WIN_MINIMIZE => {
                if let Some(idx) = self.app_idx_by_surface(u32::from(value)) {
                    if !self.win_control_allowed(idx, sender_sid) {
                        return;
                    }
                    if self.apps[idx].intent_level == wire::WIN_LEVEL_OVERLAY {
                        // OSK X (RFC-0075 Phase 8c): dismiss the band — it
                        // stays hidden until the next field tap re-announces.
                        self.osk_dismissed = true;
                        self.update_osk_visibility();
                    } else {
                        self.start_minimize_transition(idx);
                    }
                } else {
                    let _ = debug_println("WINDOWD: control win (no window for id)");
                }
            }
            wire::CONTROL_WIN_CLOSE => {
                if let Some(idx) = self.app_idx_by_surface(u32::from(value)) {
                    if !self.win_control_allowed(idx, sender_sid) {
                        return;
                    }
                    self.start_close_transition(idx);
                } else {
                    let _ = debug_println("WINDOWD: control win (no window for id)");
                }
            }
            wire::CONTROL_WIN_MODE => {
                let (sid, mode) = (u32::from(value >> 4), value & 0x0F);
                if let Some(idx) = self.app_idx_by_surface(sid) {
                    if !self.win_control_allowed(idx, sender_sid) {
                        return;
                    }
                    self.apply_window_mode(idx, mode);
                } else {
                    let _ = debug_println("WINDOWD: control win (no window for id)");
                }
            }
            // TASK-0074 D4: the ONE modal verb — `value = sid << 4 | on`.
            // Own-window gated; the flag only steers the routing loops
            // (`modal_gate`), nothing is drawn. The proof rungs ride on the
            // on/off edges (`SELFTEST: ui v10 dialog ok` after a round trip,
            // `… live modal ok` when live presses landed in between).
            wire::CONTROL_WIN_MODAL => {
                let (sid, on) = (u32::from(value >> 4), value & 1 == 1);
                // The sender's own window: an app slot, or the DESKTOP surface
                // (the shell's modal — it has no sibling windows to gate, the
                // flag records the round trip; `taskbar_gate` is its own-window
                // law). Fail-closed on no match or a foreign sid.
                let changed = if let Some(idx) = self.app_idx_by_surface(sid) {
                    if !self.win_control_allowed(idx, sender_sid) {
                        return;
                    }
                    let changed = self.apps[idx].app_modal != on;
                    self.apps[idx].app_modal = on;
                    changed
                } else if self.desktop_surface_id == Some(sid) {
                    if !matches!(
                        crate::control_gate::taskbar_gate(self.desktop_owner_sid, sender_sid),
                        crate::control_gate::GateVerdict::Allow
                    ) {
                        let _ = debug_println("WINDOWD: control win REJECT (desktop modal)");
                        return;
                    }
                    let changed = self.desktop_modal != on;
                    self.desktop_modal = on;
                    changed
                } else {
                    let _ = debug_println("WINDOWD: control win (no window for id)");
                    return;
                };
                if changed {
                    let _ = debug_println(&crate::markers::win_modal_marker(sid, on));
                    let live = self.state.pointer_route_live || self.state.keyboard_route_live;
                    let (dialog, live_ok) = self.modal_proof.edge(on, live);
                    if dialog {
                        let _ = debug_println(crate::markers::SELFTEST_UI_V10_DIALOG_OK_MARKER);
                    }
                    if live_ok {
                        let _ = debug_println(crate::markers::SELFTEST_UI_V10_LIVE_MODAL_OK_MARKER);
                    }
                }
            }
            // TASK-0066: the ONE tiling verb — `value = sid << 4 | zones::CODE_*`
            // (a zone, Return or an arrangement); own-window gated.
            wire::CONTROL_WIN_ZONE => {
                let (sid, code) = (u32::from(value >> 4), value & 0x0F);
                if let Some(idx) = self.app_idx_by_surface(sid) {
                    if !self.win_control_allowed(idx, sender_sid) {
                        return;
                    }
                    self.apply_zone_code(idx, code);
                } else {
                    let _ = debug_println("WINDOWD: control win (no window for id)");
                }
            }
            wire::CONTROL_WIN_MOVE => {
                // The chromeless-window drag handle: the app's own chrome row
                // took the press and asks windowd to move the window. Anchor
                // the drag at the CURRENT cursor — the press that triggered
                // this — and let the ordinary pointer path do the rest
                // (`drag_to` clamps to the status-bar envelope, release runs
                // the edge snap and ends the drag). Fullscreen windows do not
                // move; the app's zoom control is the way out of fullscreen.
                //
                // GATED on the primary button still being DOWN: this control
                // arrives asynchronously (app dispatch → IPC), so a plain
                // CLICK on the chrome row could start a drag AFTER its own
                // release was already processed — the window then stuck to
                // the cursor with the button up, and the NEXT click's release
                // ran the edge snap wherever the pointer happened to be
                // (top edge = surprise fullscreen).
                if let Some(idx) = self.app_idx_by_surface(u32::from(value)) {
                    if !self.win_control_allowed(idx, sender_sid) {
                        return;
                    }
                    let wid = crate::window_scene::WindowId::App(idx as u8);
                    if !self.windows.is_fullscreen(wid) {
                        self.raise_window(wid);
                        if self.state.launcher_click_visible {
                            self.apps[idx].win.begin_drag(self.state.cursor_x, self.state.cursor_y);
                        }
                    }
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

    /// RFC-0086 own-window gate for `CONTROL_WIN_*`: the kernel sender id
    /// must match the window's captured `owner_sid` (`control_gate`,
    /// host-tested reject tables). Distinct markers per reject reason —
    /// bounded by verb rate, and a reject is always a bug or an attack.
    fn win_control_allowed(&self, idx: usize, sender_sid: u64) -> bool {
        match crate::control_gate::own_window_gate(self.apps[idx].owner_sid, sender_sid) {
            crate::control_gate::GateVerdict::Allow => true,
            crate::control_gate::GateVerdict::RejectSidZero => {
                let _ = debug_println("WINDOWD: control win REJECT (sid 0)");
                false
            }
            crate::control_gate::GateVerdict::RejectForeign => {
                let _ = debug_println("WINDOWD: control win REJECT (foreign sender)");
                false
            }
        }
    }

    /// Switch the shell to the product matching a `PROFILE_*` wire tag
    /// (tablet ⇄ desktop). APPLY-ONLY (RFC-0083): settingsd owns
    /// `ui.shell.mode` — this runs on its watch events and never persists.
    /// No-op when the profile already matches (idempotent — the registration
    /// burst is the boot restore path).
    pub(crate) fn set_shell_profile_wire(&mut self, profile: u8) {
        use nexus_display_proto::client_surface as wire;
        let product = if profile == wire::PROFILE_TABLET {
            "tablet"
        } else {
            // The `default` product carries the desktop profile/shell.
            "default"
        };
        if self.shell_profile_wire() == profile {
            return;
        }
        let cfg = systemui::shell_config_for(product);
        self.apply_shell_config(cfg);
        // The work area changed with the profile (taskbar): tiled windows follow (TASK-0066).
        self.reflow_tiled();
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
            // The declared abilitymgr pair (TASK-0324 P4a; it answers on its own endpoint) —
            // no route ask, no retry loop (P7-b).
            if self.abilitymgr_client.is_none() {
                let leg = nexus_service_topology::slots::windowd::ABILITYMGR;
                self.abilitymgr_client =
                    nexus_ipc::KernelClient::new_with_slots(leg.send, leg.recv).ok();
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
            if client.send(&req, nexus_ipc::Wait::Blocking).is_err() {
                let _ = debug_println("windowd: FAIL launch send");
                self.end_cursor_wait();
                return;
            }
            // The reply arrives on abilitymgr's response endpoint — a waitset member of the
            // compositor loop (TASK-0324 P7-c): `drain_launch_replies` handles it. No poll.
        }
    }

    /// Drains abilitymgr's launch replies `[A,M,ver,OP_LAUNCH|0x80,status]` (waitset wake):
    /// a refused launch ends the pointer's wait ring at once — the surface it waits for will
    /// never arrive — instead of the ring running out its failsafe.
    #[cfg(nexus_env = "os")]
    pub(crate) fn drain_launch_replies(&mut self) {
        let mut denied = 0u32;
        if let Some(client) = self.abilitymgr_client.as_ref() {
            let mut rsp = [0u8; 16];
            while let Ok(n) = client.recv_into(nexus_ipc::Wait::NonBlocking, &mut rsp) {
                if n >= 5 && rsp[3] == 0x81 && rsp[4] != 0 {
                    denied += 1;
                }
            }
        }
        for _ in 0..denied {
            let _ = debug_println("windowd: launch denied");
            self.end_cursor_wait();
        }
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
            let (data, rows) = systemui::wallpaper_rle_for(mode == crate::theme::ThemeMode::Dark);
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
                    let _ =
                        client.send(&[nexus_display_proto::OP_WALLPAPER_DIRTY], Wait::NonBlocking);
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
        // The app-client window chrome follows the theme (re-rendered from
        // `self.theme()`); app/shell CONTENT re-themes via the RFC-0083
        // presentation snapshot (reemit, no remount) — pumped after the
        // generation bump below.
        for slot in self.apps.iter_mut() {
            slot.win.surface_dirty = true;
            slot.surface_dirty_rows = None; // re-theme: full re-blit (chrome too)
            slot.win.blur_valid = false;
        }
        self.queue_full_frame_damage();
        // RFC-0083: the packed theme byte is part of the presentation
        // snapshot — every bound channel owes the new state.
        self.presentation_changed();
        let _ = debug_println(&alloc::format!("uitheme: switched (to={})", mode.as_str()));
    }
}
