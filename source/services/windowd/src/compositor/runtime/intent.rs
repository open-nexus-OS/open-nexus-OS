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
            // Default body size for a NEW window: the next free slot's default
            // frame (a re-creating client will re-negotiate through create).
            let idx = self.free_app_index().unwrap_or(0);
            let win = &self.apps[idx].win;
            (win.w as u16, win.h.saturating_sub(win.title_h) as u16)
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
                let _ = debug_println("WINDOWD: FAIL intent reply (no channel for nonce)");
            }
        }
        #[cfg(not(nexus_env = "os"))]
        let _ = rect;
        let _ = debug_println(&alloc::format!(
            "WINDOWD: surface intent style={style} level={level} mode={mode} -> {rw}x{rh}"
        ));
    }

    /// The work-area HEIGHT for fullscreen/maximized APP windows: the display
    /// minus the desktop taskbar (desktop profile only — "nicht über die
    /// Taskleiste"); the tablet dock is overlaid, so fullscreen reaches the
    /// bottom edge there. The top is NOT inset — windows sit BEHIND the shell
    /// top bar (it composites above them, `SHELL_TOPBAR_H`).
    pub(crate) fn work_area_h(&self) -> u32 {
        use nexus_display_proto::client_surface as wire;
        if self.shell_profile_wire() == wire::PROFILE_DESKTOP {
            self.mode.height.saturating_sub(super::SHELL_TASKBAR_H)
        } else {
            self.mode.height
        }
    }
}
