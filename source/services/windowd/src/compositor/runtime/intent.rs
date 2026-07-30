// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: windowd compositor runtime — the `OP_SURFACE_INTENT` geometry
//! handshake (WM owns geometry: intent ⟂ policy → composed content rect,
//! answered on the asking client's event channel) and the work-area rule
//! (moved out of `app_window.rs`, structure gate).
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: presentation resolution is host-tested
//! (`surface_presentation`); the wire path rides the QEMU boot proofs.

use super::*;
use nexus_display_proto::client_surface as wire;

impl DisplayServerRuntime {
    /// Window intent (`OP_SURFACE_INTENT`, sent before create): store the
    /// style/level/mode and answer the composed **content rect** the app sizes
    /// its surface VMO to (the WM owns geometry — no display-mode query). Under
    /// the v1 Desktop policy a `desktop`/`fullscreen` surface fills the display;
    /// otherwise it gets the default window body size. Reply rides the app event
    /// channel; if it is not attached yet the app's bounded wait falls back.
    pub(crate) fn handle_surface_intent(&mut self, frame: &[u8]) {
        let Some((style, level, mode, resizable, nonce)) = wire::decode_surface_intent(frame)
        else {
            return;
        };
        // STATELESS: the reply is computed from THIS frame's intent only.
        // Storing it here poisoned the floating window's `app_intent_*` when a
        // desktop app (shell/greeter) asked while a window was open — the
        // create carries the intent atomically, so nothing needs it stored.
        let p = crate::surface_presentation::WindowPresentation::resolve(
            style,
            level,
            mode,
            resizable,
            self.windowing_policy,
        );
        let (rw, rh) = if p.docked_bottom {
            // OVERLAY (RFC-0075 Phase 2): the WM owns the OSK band geometry —
            // full display width, fixed band height, docked bottom.
            (self.mode.width as u16, super::OSK_BAND_H as u16)
        } else if p.full_screen {
            // Same work-area rule as the create branch: shell/greeter span
            // the display; a fullscreen APP window gets the work-area height.
            let h = if level == wire::WIN_LEVEL_DESKTOP {
                self.mode.height
            } else {
                self.work_area_h()
            };
            (self.mode.width as u16, h as u16)
        } else {
            // Default body size for a NEW floating window: the SAME centred
            // fraction `set_window_mode(freeform)` applies (¾ wide, ⅚ tall),
            // so a window that asks before it exists and one the user later
            // flips to freeform land on the same geometry. ⅚ rather than ¾
            // for the HEIGHT: the design handoffs draw 900×600-proportioned
            // windows, and at ¾ (526 rows) a settings/file-manager sidebar
            // could not seat its full section list.
            //
            // This used to read the next free SLOT's frame, which for a slot
            // nothing has framed yet is the allocator CEILING (1280x3072) — a
            // "default size" three times taller than the display. Nothing
            // caught it because app-host never asked on this path until a
            // window declared `mode: freeform`.
            //
            // The title reserve comes from THIS intent's resolved
            // presentation — a chromeless (`plain`) window draws no WM title
            // bar, and charging it one anyway (via the free slot's default)
            // shrank every chromeless window by a bar it never shows.
            let title = if p.has_chrome { app_window::APP_TITLE_H } else { 0 };
            let w = (self.mode.width * 3 / 4).max(super::wm::MIN_WIN_W);
            let h = (self.work_area_h() * 5 / 6).max(super::wm::MIN_WIN_H);
            (w as u16, h.saturating_sub(title) as u16)
        };
        let rect = wire::encode_surface_rect(0, 0, rw, rh);
        // Reply on the ASKING client's own event channel (nonce correlation —
        // the same contract as create/events). The last-attached-channel send
        // this replaces let concurrent mounts steal each other's rect: every
        // app then mounted at the probe fallback size.
        #[cfg(nexus_env = "os")]
        {
            if let Some(slot) = self.event_channel_for(nonce) {
                let hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, rect.len() as u32);
                let _ = nexus_abi::ipc_send_v1(slot, &hdr, &rect, nexus_abi::IPC_SYS_NONBLOCK, 0);
            } else {
                // The execd attach RACES this intent (~40% of boots — the
                // 320x240-fallback/splash-hang class): PARK the composed
                // rect; `attach_app_event_channel` flushes it (bounded,
                // drop-oldest — the app's 2s wait covers the flush).
                let slot_idx =
                    self.pending_intent_replies.iter().position(Option::is_none).unwrap_or(0);
                self.pending_intent_replies[slot_idx] = Some((nonce, rect));
                let _ = debug_println("WINDOWD: intent reply parked (awaiting attach)");
            }
        }
        #[cfg(not(nexus_env = "os"))]
        let _ = rect;
        let _ = debug_println(&alloc::format!(
            "WINDOWD: surface intent style={style} level={level} mode={mode} -> {rw}x{rh}"
        ));
    }

    /// The frame a full-screen/maximized surface takes: `(y, height)`.
    ///
    /// ONE definition for the create path and the maximize path — they used to
    /// hard-code `set_frame(0, 0, …)` separately, which is why adding a status
    /// bar meant finding both.
    pub(crate) fn full_surface_frame(&self, idx: usize) -> (i32, u32) {
        use nexus_display_proto::client_surface as wire;
        let bottom = if self.apps[idx].intent_level == wire::WIN_LEVEL_DESKTOP {
            self.mode.height
        } else {
            self.work_area_h()
        };
        let g = crate::surface_presentation::bar_geometry(
            &self.app_presentation(idx),
            self.windows.is_fullscreen(crate::window_scene::WindowId::App(idx as u8)),
        );
        (g.frame_min_y, bottom.saturating_sub(g.frame_min_y as u32))
    }

    /// The status-bar rows THIS surface must keep clear at the top of its own
    /// content, shipped to the app in the (already reserved, until now always
    /// zero) `y` of `OP_SURFACE_RECT`.
    pub(crate) fn content_top_inset(&self, idx: usize) -> u32 {
        crate::surface_presentation::bar_geometry(
            &self.app_presentation(idx),
            self.windows.is_fullscreen(crate::window_scene::WindowId::App(idx as u8)),
        )
        .content_top_inset
    }

    /// The status-bar drag envelope for one app window — ONE definition for
    /// the title drag AND the top-edge resize, so the two clamps can never
    /// disagree. The grab strip is the WM title bar when the window has
    /// chrome; a chromeless window's strip is its own top chrome row, assumed
    /// bar-height (the app draws it, windowd only needs a reachable band).
    pub(super) fn drag_bounds(&self, idx: usize) -> nexus_widget_window::DragBounds {
        let title_h = self.apps[idx].win.title_h;
        nexus_widget_window::DragBounds {
            min_y: crate::surface_presentation::SHELL_TOPBAR_H as i32,
            max_grab_bottom: self.work_area_h() as i32,
            grab_h: if title_h > 0 { title_h } else { crate::surface_presentation::SHELL_TOPBAR_H },
            min_visible_w: 64,
        }
    }

    /// One drag step of an app window: `drag_to` under this window's
    /// [`drag_bounds`](Self::drag_bounds) envelope. Returns the vacated damage
    /// rect when the window moved.
    pub(super) fn drag_app_window(
        &mut self,
        idx: usize,
        cx: i32,
        cy: i32,
    ) -> Option<crate::compositor::damage::DamageRect> {
        let b = self.drag_bounds(idx);
        let (w, h) = (self.mode.width, self.mode.height);
        self.apps[idx].win.drag_to(cx, cy, w, h, b)
    }

    /// The work-area HEIGHT for fullscreen/maximized APP windows: the display
    /// minus the desktop taskbar (desktop profile only — "nicht über die
    /// Taskleiste"); the tablet dock is overlaid, so fullscreen reaches the
    /// bottom edge there. The TOP is per-surface and lives in
    /// `surface_presentation::bar_geometry`, not here: a chromeless fullscreen
    /// window still spans to y=0 (its glass runs UNDER the status bar and its
    /// content is inset instead), while a chromed or floating one starts below.
    pub(crate) fn work_area_h(&self) -> u32 {
        use nexus_display_proto::client_surface as wire;
        if self.shell_profile_wire() == wire::PROFILE_DESKTOP {
            self.mode.height.saturating_sub(super::SHELL_TASKBAR_H)
        } else {
            self.mode.height
        }
    }
}
