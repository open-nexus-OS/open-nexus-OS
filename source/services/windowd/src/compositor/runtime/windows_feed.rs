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
//! (host-tested); this file is the wire + delivery glue. The same slot facts
//! are what the compositor STATES to the lifecycle authority (TASK-0333 6d,
//! ADR-0083): one `OP_SEEN` per changed window to abilitymgr (`crate::seen`,
//! pure), NONBLOCK, retried on the frame loop — the compositor never waits on
//! the authority and never decides lifecycle itself.
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
    /// The sightings the lifecycle authority took (6d); a sighting not yet taken is owed.
    seen: crate::seen::SeenTable,
    seen_owed: bool,
    /// This compositor's start is not told yet (RFC-0112): it goes before any sighting.
    up_owed: bool,
    /// Reused per pass (the slots grow, RFC-0114 §7): every slot's facts, the sightings due,
    /// the ones the authority took.
    slots: Vec<crate::window_feed::SlotInfo>,
    due: Vec<crate::seen::Due>,
    taken: Vec<crate::seen::Due>,
}

impl FeedState {
    pub(super) fn new() -> Self {
        Self {
            last: [0u8; feed::SURFACE_WINDOWS_FRAME_MAX],
            len: 0,
            owed: false,
            seen: crate::seen::SeenTable::default(),
            seen_owed: false,
            up_owed: true,
            slots: Vec::new(),
            due: Vec::new(),
            taken: Vec::new(),
        }
    }
}

/// The feed listed fewer windows than are open (its frame's bound) — said once.
static FEED_BOUND_SAID: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

impl DisplayServerRuntime {
    /// Every app slot reduced to the facts the feed and the sightings are built from.
    fn slot_infos(&self, slots: &mut Vec<crate::window_feed::SlotInfo>) {
        slots.clear();
        for (i, slot) in self.apps.iter().enumerate() {
            let id = crate::window_scene::WindowId::App(i as u16);
            slots.push(crate::window_feed::SlotInfo {
                owner_sid: slot.owner_sid,
                // A bound surface + a visible window state = OPEN (minimized
                // windows stay visible-but-minimized in `window_scene`).
                open: slot.surface_id.is_some() && self.windows.is_visible(id),
                minimized: self.windows.is_minimized(id),
                focused: self.windows.is_top(id),
                zone: slot.zone.code(),
            });
        }
    }

    /// The current window set as a wire frame (`None` = encode refused): the first
    /// `WINDOWS_MAX` open windows (the frame's bound — said once when more are open).
    fn window_feed_frame(
        slots: &[crate::window_feed::SlotInfo],
    ) -> Option<([u8; feed::SURFACE_WINDOWS_FRAME_MAX], usize)> {
        let mut entries = [feed::WindowEntry::default(); feed::WINDOWS_MAX];
        let n = crate::window_feed::build_window_set(slots, &mut entries);
        let open = slots.iter().filter(|s| s.open && s.owner_sid != 0).count();
        if open > n && !FEED_BOUND_SAID.swap(true, core::sync::atomic::Ordering::Relaxed) {
            let _ = debug_println(&alloc::format!(
                "windowd: window feed bound (listed={n} open={open})"
            ));
        }
        feed::encode_surface_windows(&entries[..n])
    }

    /// The compositor's sightings for the lifecycle authority (TASK-0333 6d, ADR-0083): one
    /// `OP_SEEN` per app window whose sighting changed, NONBLOCK on the declared abilitymgr
    /// leg. A send the queue refuses stays owed and goes with the next pass; the compositor
    /// never parks on the authority.
    #[cfg(nexus_env = "os")]
    fn publish_seen(&mut self, slots: &[crate::window_feed::SlotInfo]) {
        use nexus_ipc::Client as _;
        let mut due = core::mem::take(&mut self.windows_feed.due);
        let mut taken = core::mem::take(&mut self.windows_feed.taken);
        self.windows_feed.seen.due(slots, &mut due);
        taken.clear();
        self.windows_feed.seen_owed = false;
        if !due.is_empty() || self.windows_feed.up_owed {
            let _ = self.abilitymgr(); // bound once
        }
        // RFC-0112: this compositor's start first — what an earlier one saw is void, the
        // frozen are thawed to rejoin. No sighting overtakes it.
        if self.windows_feed.up_owed {
            let frame = crate::seen::encode_compositor_up();
            let told = self
                .abilitymgr_client
                .as_ref()
                .is_some_and(|c| c.send(&frame, nexus_ipc::Wait::NonBlocking).is_ok());
            self.windows_feed.up_owed = !told;
            if !told {
                self.windows_feed.seen_owed = true;
                self.windows_feed.due = due;
                self.windows_feed.taken = taken;
                return;
            }
        }
        match self.abilitymgr_client.as_ref() {
            Some(client) => {
                for d in &due {
                    let frame = crate::seen::encode_seen(d.owner_sid, d.seen);
                    if client.send(&frame, nexus_ipc::Wait::NonBlocking).is_ok() {
                        self.windows_feed.seen.commit(*d);
                        taken.push(*d);
                    } else {
                        self.windows_feed.seen_owed = true;
                    }
                }
            }
            None => self.windows_feed.seen_owed = !due.is_empty(),
        }
        // TASK-0333 6e-1 (ADR-0086): the window itself hears the same sighting — on `hidden` the
        // app writes its `@persist` snapshot while the grace runs. The authority's send decides
        // what is due; the app's copy rides the taken ones (a closed window has no channel).
        for d in &taken {
            let seen = match d.seen {
                crate::seen::Seen::Focused => nexus_display_proto::surface_seen::SEEN_FOCUSED,
                crate::seen::Seen::Visible => nexus_display_proto::surface_seen::SEEN_VISIBLE,
                crate::seen::Seen::Hidden => nexus_display_proto::surface_seen::SEEN_HIDDEN,
                crate::seen::Seen::Gone => continue,
            };
            let frame = nexus_display_proto::surface_seen::encode_surface_seen(seen);
            let _ = self.send_app_frame(d.slot, &frame);
        }
        self.windows_feed.due = due;
        self.windows_feed.taken = taken;
    }

    /// The declared abilitymgr leg (TASK-0324 P4a; it answers on its own endpoint) — bound
    /// once, no route ask, no retry loop (P7-b). Shared by the launch request and the sightings.
    #[cfg(nexus_env = "os")]
    pub(super) fn abilitymgr(&mut self) -> Option<&nexus_ipc::KernelClient> {
        if self.abilitymgr_client.is_none() {
            let leg = nexus_service_topology::slots::windowd::ABILITYMGR;
            self.abilitymgr_client =
                nexus_ipc::KernelClient::new_with_slots(leg.send, leg.recv).ok();
        }
        self.abilitymgr_client.as_ref()
    }

    /// The window set changed (open / close / minimize / restore / focus) —
    /// re-encode and push. Deduped: an unchanged encoding sends nothing, so
    /// per-frame callers are free. Called again on desktop (re)bind, which is
    /// what makes the feed retained rather than event-sourced.
    pub(crate) fn push_window_set(&mut self) {
        #[cfg(nexus_env = "os")]
        {
            let mut slots = core::mem::take(&mut self.windows_feed.slots);
            self.slot_infos(&mut slots);
            self.publish_seen(&slots);
            let made = Self::window_feed_frame(&slots);
            self.windows_feed.slots = slots;
            let Some((frame, n)) = made else { return };
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
        if self.windows_feed.owed || self.windows_feed.seen_owed || self.windows_feed.up_owed {
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
        let id = crate::window_scene::WindowId::App(idx as u16);
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
