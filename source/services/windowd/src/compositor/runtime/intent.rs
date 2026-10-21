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
        let auto = mode == wire::WIN_MODE_AUTO && level == wire::WIN_LEVEL_NORMAL;
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
            // The surface IS the frame (TASK-0337): no title reserve, the app draws its bar.
            let w = (self.mode.width * 3 / 4).max(super::wm::MIN_WIN_W);
            let h = (self.work_area_h() * 5 / 6).max(super::wm::MIN_WIN_H);
            (w as u16, h as u16)
        };
        // A window that declares no size (`mode: auto`) gets the WM's small utility frame
        // (TASK-0336: the WM owns every window's geometry — the app asks, never assumes).
        let (rw, rh) = if auto && !p.full_screen && !p.docked_bottom {
            (super::app_window::AUTO_WINDOW.0 as u16, super::app_window::AUTO_WINDOW.1 as u16)
        } else {
            (rw, rh)
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
        // A fresh floating window shows its starting surface now (TASK-0336): the spawn, the
        // mount and the first frame happen behind a window that is already there.
        #[cfg(nexus_env = "os")]
        if !p.full_screen && !p.docked_bottom && level == wire::WIN_LEVEL_NORMAL {
            if let Some(ch) = self.event_channel_for(nonce) {
                if !self.apps.iter().any(|a| a.event_channel == Some(ch)) {
                    self.begin_starting_surface(
                        ch,
                        (u32::from(rw), u32::from(rh)),
                        (style, level, mode, resizable),
                    );
                }
            }
        }
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
    /// disagree. The grab strip is the window's own top chrome row (the app draws
    /// it, TASK-0337), assumed bar-height: windowd only needs a reachable band.
    pub(super) fn drag_bounds(&self, _idx: usize) -> nexus_widget_window::DragBounds {
        nexus_widget_window::DragBounds {
            min_y: crate::surface_presentation::SHELL_TOPBAR_H as i32,
            max_grab_bottom: self.work_area_h() as i32,
            grab_h: crate::surface_presentation::SHELL_TOPBAR_H,
            min_visible_w: 64,
        }
    }

    /// One drag step of an app window (TASK-0336 P3): the window's frame stays where the
    /// drag began; the pointer's offset, clamped under this window's
    /// [`drag_bounds`](Self::drag_bounds) envelope, rides in the next present as the window's
    /// transform (TASK-0338: owed, coalesced to the frame clock — the latest offset when the
    /// present is built) — the retained layers move on the GPU, no re-render and no re-blur per
    /// pointer move. Returns whether the offset changed.
    pub(super) fn drag_app_window(&mut self, idx: usize, cx: i32, cy: i32) -> bool {
        let b = self.drag_bounds(idx);
        let w = self.mode.width;
        let Some((nx, ny)) = self.apps[idx].win.drag_target(cx, cy, w, b) else { return false };
        let win = &self.apps[idx].win;
        let offset = (nx - win.x, ny - win.y);
        if self.apps[idx].drag_offset == Some(offset) {
            return false;
        }
        self.apps[idx].drag_offset = Some(offset);
        self.owe_frame();
        true
    }

    /// The drag ends: the frame moves to where the transform showed it (clamped as every
    /// step was) and the offset leaves the transform — both in the next present
    /// (`Command::LayerTransform` beside the moved layers, atomic), which draws the window at
    /// its place and blurs its glass once (the planner's blur at rest).
    pub(super) fn end_app_drag(&mut self, idx: usize, cx: i32, cy: i32) {
        let b = self.drag_bounds(idx);
        let (w, h) = (self.mode.width, self.mode.height);
        let old = self.apps[idx].win.damage_rect(w, h);
        let _ = self.apps[idx].win.drag_to(cx, cy, w, h, b);
        self.apps[idx].drag_offset = None;
        self.apps[idx].win.end_drag();
        let new = self.apps[idx].win.damage_rect(w, h);
        self.queue_gpu_blit_rect(old.merge(new));
        self.invalidate_blur_over(old.merge(new), Some(idx));
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
