// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: windowd → clipboardd focus truth (RFC-0094 `OP_FOCUS`, TASK-0067).
//! clipboardd gates every read on WHO the user is looking at: the owner of the
//! focused window (the paste), the desktop-surface owner (the shell's search)
//! and the IME overlay's owner (the on-screen keyboard's clipboard). windowd
//! is the only party that knows those three kernel sids, so it pushes them —
//! clipboardd never asks.
//!
//! Retained latest-wins like the window feed: recomputed on the frame pass,
//! deduped against the last SENT triple, NONBLOCK with an owed flag retried on
//! the next pass — a busy authority never wedges the compositor, and no focus
//! change site can forget to push (there is no site: the pass is the push).
//! windowd draws nothing and decides nothing here; it states facts.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: the owner choice is the pure `clipboard_owners`; the wire
//!   path via the QEMU marker `clipboardd: focus truth live`.
//! RFC: docs/rfcs/RFC-0094-content-transfer-v1-clipboardd.md

use super::*;

/// Delivery state of the focus truth.
pub(super) struct ClipFocusState {
    /// The last SENT (focused, desktop, ime) owners — the dedupe key. Starts at
    /// clipboardd's own initial truth (nobody), so nothing is sent until an
    /// owner exists.
    last: (u64, u64, u64),
    /// A send is still owed (the authority's queue was full).
    owed: bool,
    #[cfg(nexus_env = "os")]
    client: Option<nexus_ipc::KernelClient>,
}

impl ClipFocusState {
    pub(super) const fn new() -> Self {
        Self {
            last: (0, 0, 0),
            owed: false,
            #[cfg(nexus_env = "os")]
            client: None,
        }
    }
}

impl DisplayServerRuntime {
    /// The owners a clipboard read is allowed for right now: the focused
    /// window's (the topmost when nothing was raised yet — what a paste would
    /// reach), the desktop surface's and the live IME overlay's. 0 = none.
    pub(super) fn clipboard_owners(&self) -> (u64, u64, u64) {
        use crate::window_scene::WindowId;
        let focused = match self.windows.focused().or_else(|| self.windows.top()) {
            Some(WindowId::App(i)) => {
                let slot = &self.apps[usize::from(i)];
                if slot.surface_id.is_some() {
                    slot.owner_sid
                } else {
                    0
                }
            }
            Some(WindowId::Desktop) => self.desktop_owner_sid,
            None => 0,
        };
        let ime = self.osk_idx().map_or(0, |i| self.apps[i].owner_sid);
        (focused, self.desktop_owner_sid, ime)
    }

    /// One delivery pass (frame loop, cheap): push the owners when they
    /// changed or a send is owed.
    pub(crate) fn push_clipboard_focus(&mut self) {
        let owners = self.clipboard_owners();
        if owners == self.clip_focus.last && !self.clip_focus.owed {
            return;
        }
        self.clip_focus.last = owners;
        #[cfg(nexus_env = "os")]
        {
            if self.clip_focus.client.is_none() {
                // The declared clipboardd leg (init wires it from windowd's spec).
                let leg = nexus_service_topology::slots::windowd::CLIPBOARDD;
                self.clip_focus.client =
                    nexus_ipc::KernelClient::new_with_slots(leg.send, leg.recv).ok();
            }
            let Some(client) = self.clip_focus.client.as_ref() else {
                self.clip_focus.owed = true;
                return;
            };
            let frame = nexus_wire::clipboardd::encode_focus(owners.0, owners.1, owners.2);
            use nexus_ipc::Client as _;
            let sent = client.send(&frame, Wait::NonBlocking).is_ok();
            self.clip_focus.owed = !sent;
        }
    }
}
