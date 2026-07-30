// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: RFC-0086 window feed — windowd pushes the running app-window set
//! (`OP_SURFACE_WINDOWS`) to the DESKTOP surface's event channel so the shell
//! taskbar/dock can render running/minimized/focused state and activate a
//! window. Retained latest-wins, exactly like the RFC-0083 presentation
//! snapshot: change-driven, deduped against the last-sent bytes, NONBLOCK
//! with an OWED flag retried on the frame loop — a slow shell never wedges
//! the compositor and never permanently misses state.
//!
//! The pure "which windows" decision lives in `crate::window_feed`
//! (host-tested); this file is the wire + delivery glue.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: pure core in `crate::window_feed`; wire path via the QEMU
//! marker `windowd: windows push (n=…)`.
//! RFC: docs/rfcs/RFC-0086-shell-taskbar-window-feed.md

use super::*;
use nexus_display_proto::surface_windows as feed;

/// Retained delivery state for the feed: the LAST SENT frame (the dedupe
/// key) and whether a send is still owed (channel full, or the desktop had
/// not bound yet — retried on the frame loop).
pub(super) struct FeedState {
    last: [u8; feed::SURFACE_WINDOWS_FRAME_MAX],
    len: usize,
    pub(super) owed: bool,
}

impl FeedState {
    pub(super) const fn new() -> Self {
        Self { last: [0u8; feed::SURFACE_WINDOWS_FRAME_MAX], len: 0, owed: false }
    }
}

impl DisplayServerRuntime {
    /// The current window set as a wire frame (`None` = encode refused).
    fn window_feed_frame(&self) -> Option<([u8; feed::SURFACE_WINDOWS_FRAME_MAX], usize)> {
        let mut slots =
            [crate::window_feed::SlotInfo::default(); crate::window_scene::MAX_APP_WINDOWS];
        for (i, slot) in self.apps.iter().enumerate() {
            let id = crate::window_scene::WindowId::App(i as u8);
            slots[i] = crate::window_feed::SlotInfo {
                owner_sid: slot.owner_sid,
                // A bound surface + a visible window state = OPEN (minimized
                // windows stay visible-but-minimized in `window_scene`).
                open: slot.surface_id.is_some() && self.windows.is_visible(id),
                minimized: self.windows.is_minimized(id),
                focused: self.windows.is_top(id),
            };
        }
        let mut entries = [feed::WindowEntry::default(); feed::WINDOWS_MAX];
        let n = crate::window_feed::build_window_set(&slots, &mut entries);
        feed::encode_surface_windows(&entries[..n])
    }

    /// The window set changed (open / close / minimize / restore / focus) —
    /// re-encode and push. Deduped: an unchanged encoding sends nothing, so
    /// per-frame callers are free. Called again on desktop (re)bind, which is
    /// what makes the feed retained rather than event-sourced.
    pub(crate) fn push_window_set(&mut self) {
        #[cfg(nexus_env = "os")]
        {
            let Some((frame, n)) = self.window_feed_frame() else { return };
            if self.windows_feed.len == n && self.windows_feed.last[..n] == frame[..n] {
                // Same state as last SENT: nothing owed, nothing to send.
                if !self.windows_feed.owed {
                    return;
                }
            } else {
                self.windows_feed.last[..n].copy_from_slice(&frame[..n]);
                self.windows_feed.len = n;
            }
            let Some(slot) = self.desktop_channel else {
                // No desktop surface yet: the state is recorded, the bind
                // path re-pushes it (retained, not lost).
                self.windows_feed.owed = true;
                return;
            };
            let hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, n as u32);
            let sent =
                nexus_abi::ipc_send_v1(slot, &hdr, &frame[..n], nexus_abi::IPC_SYS_NONBLOCK, 0)
                    .is_ok();
            self.windows_feed.owed = !sent;
            if sent {
                // Marker AFTER a real send (count = entries, not bytes; the
                // count byte follows the 4-byte envelope header).
                let count = frame.get(4).copied().unwrap_or(0);
                let _ = debug_println(&alloc::format!("windowd: windows push (n={count})"));
            }
        }
    }

    /// Frame-loop retry for an owed push (the channel was full or not bound
    /// yet). Cheap: returns immediately unless something is actually owed.
    pub(crate) fn pump_window_feed(&mut self) {
        if self.windows_feed.owed {
            self.push_window_set();
        }
    }

    /// `OP_SURFACE_TASKBAR` (RFC-0086): the SHELL asks windowd to activate a
    /// window it does not own. Three fail-closed gates in order — sender
    /// identity (must be the desktop-surface owner), frame validity, target
    /// resolution by owner sid. Every reject emits its own marker: a reject
    /// here is always a bug or an attack, never routine.
    pub(crate) fn handle_surface_taskbar(&mut self, frame: &[u8], sender_sid: u64) {
        use crate::control_gate::{taskbar_gate, GateVerdict};
        match taskbar_gate(self.desktop_owner_sid, sender_sid) {
            GateVerdict::Allow => {}
            GateVerdict::RejectSidZero => {
                let _ = debug_println("WINDOWD: taskbar REJECT (sid 0)");
                return;
            }
            GateVerdict::RejectForeign => {
                let _ = debug_println("WINDOWD: taskbar REJECT (not the desktop owner)");
                return;
            }
        }
        let Some((verb, target_sid)) = feed::decode_surface_taskbar(frame) else {
            let _ = debug_println("WINDOWD: FAIL taskbar (malformed)");
            return;
        };
        let Some(idx) = self
            .apps
            .iter()
            .position(|s| s.surface_id.is_some() && s.owner_sid == target_sid && target_sid != 0)
        else {
            let _ = debug_println("windowd: taskbar activate miss");
            return;
        };
        let id = crate::window_scene::WindowId::App(idx as u8);
        match verb {
            feed::TASKBAR_ACTIVATE => {
                if self.windows.is_minimized(id) {
                    // Restore from the taskbar: fly in from the taskbar band
                    // centre (windowd owns the bar HEIGHT as work-area policy,
                    // never the shell's tile positions).
                    let (cx, cy) = self.taskbar_anchor();
                    self.start_restore_transition(id, cx, cy);
                } else {
                    self.raise_window(id);
                }
                let _ = debug_println("windowd: taskbar activate ok");
            }
            feed::TASKBAR_MINIMIZE => {
                // Reserved verb (RFC-0086): click-toggle. Decoded and gated,
                // but deliberately not acted on in v1 — reported, not silent.
                let _ = debug_println("windowd: taskbar minimize (reserved, no-op)");
            }
            other => {
                let _ = debug_println(&alloc::format!("WINDOWD: taskbar unknown verb={other}"));
            }
        }
    }

    /// The minimize/restore animation anchor: the centre of the taskbar band
    /// (`SHELL_TASKBAR_H` is windowd's declared work-area reservation). The
    /// shell knows where its tiles are; windowd deliberately does not — a
    /// per-tile target would make the compositor depend on shell layout.
    pub(crate) fn taskbar_anchor(&self) -> (f32, f32) {
        (
            self.mode.width as f32 / 2.0,
            self.mode.height as f32 - super::SHELL_TASKBAR_H as f32 / 2.0,
        )
    }
}
