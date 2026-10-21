// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The one funnel for every frame windowd sends a client (an app's or the desktop's
//! event channel) — ADR-0075: the compositor never waits on a client. A frame goes straight
//! into the client's queue when nothing of that client waits before it; when the queue is full
//! it waits in the client's bounded outbox (`crate::client_outbox`), merged only if it is state
//! the next frame supersedes; windowd arms the kernel's writable signal for the channel
//! (`waitset_arm_writable`, RFC-0033) and goes on composing. The client's next receive wakes
//! windowd, and every loop pass sends what waits, in order. A send that finds no receiver is
//! the client's death (TASK-0333 6a: its window closes by name).
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `client_outbox` unit tests (order, merge, bounds); the kernel half is
//!   `KSELFTEST: waitset writable ok` every boot; the line `clients(behind= queued_max=
//!   dropped=)` on `windowd: loop` on every lane
//! ADR: docs/adr/0075-the-compositor-never-waits-on-a-client.md

use super::DisplayServerRuntime;
use crate::client_outbox::{Attempt, Flush, Kind};
use crate::client_surface::Delivery;

/// What one frame for a client came to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ClientSend {
    /// In the client's queue.
    Sent,
    /// Waiting in the client's outbox: it leaves, in order, as the client drains.
    Queued,
    /// Dropped: the client is far behind (its outbox full of ORDERED frames) or every outbox
    /// is bound to a client behind — counted on the loop line, the client named there.
    Refused,
    /// The peer is gone: no receiver exists any more, or the endpoint died.
    Dead,
}

impl ClientSend {
    /// Accepted for delivery (now or in order later).
    pub(super) const fn accepted(self) -> bool {
        matches!(self, ClientSend::Sent | ClientSend::Queued)
    }
}

/// One non-blocking send into the client's queue — never a park.
fn try_send(channel: u32, frame: &[u8]) -> Attempt {
    let hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, frame.len() as u32);
    match nexus_abi::ipc_send_v1(channel, &hdr, frame, nexus_abi::IPC_SYS_NONBLOCK, 0) {
        Ok(_) => Attempt::Sent,
        // A full queue (the kernel says when it has room) — or the receiver's byte budget.
        Err(nexus_abi::IpcError::QueueFull) => Attempt::Full,
        Err(nexus_abi::IpcError::NoSpace) => Attempt::Budget,
        // No receiver, no endpoint, or a channel windowd no longer holds: it never drains.
        Err(_) => Attempt::Dead,
    }
}

impl DisplayServerRuntime {
    /// One frame for `channel` (ADR-0075): into the client's queue now, or into its outbox
    /// behind what already waits — never a park on the client.
    pub(super) fn client_send(
        &mut self,
        channel: u32,
        frame: &[u8],
        delivery: Delivery,
    ) -> ClientSend {
        let kind = match crate::client_outbox::latest_key(frame) {
            Some(key) if !delivery.ordered() => Kind::Latest(key),
            _ => Kind::Ordered,
        };
        if !self.outboxes.waiting(channel) {
            match try_send(channel, frame) {
                Attempt::Sent => return ClientSend::Sent,
                Attempt::Dead => return ClientSend::Dead,
                Attempt::Full | Attempt::Budget => {}
            }
        }
        if !self.outboxes.bind(channel).is_some_and(|o| o.push(frame, kind)) {
            return ClientSend::Refused;
        }
        match self.flush_client(channel) {
            Flush::Drained => ClientSend::Sent,
            Flush::Full => ClientSend::Queued,
            Flush::Dead => ClientSend::Dead,
        }
    }

    /// Sends what waits for `channel`, in order; when its queue is full again, arms the
    /// kernel's writable signal for it (the client's next receive wakes this loop).
    fn flush_client(&mut self, channel: u32) -> Flush {
        let waitset = self.outboxes.waitset();
        let Some(outbox) = self.outboxes.get_mut(channel) else { return Flush::Drained };
        let mut queue_full = false;
        let verdict = outbox.flush(|frame| {
            let attempt = try_send(channel, frame);
            queue_full = attempt == Attempt::Full;
            attempt
        });
        match verdict {
            // The queue's room is what the kernel reports; a byte-budget stop waits a pass.
            Flush::Full if queue_full => {
                if let Some(ws) = waitset {
                    let _ = nexus_abi::waitset_arm_writable(ws, channel);
                }
            }
            Flush::Full => {}
            Flush::Dead => self.outboxes.forget(channel),
            Flush::Drained => {}
        }
        verdict
    }

    /// Every loop pass, before the wait: what waits leaves as the clients drained; a client
    /// found gone closes its window by name.
    pub(crate) fn flush_client_outboxes(&mut self) {
        let mut behind = [0u32; crate::client_outbox::CLIENTS_MAX];
        let mut n = 0;
        for (slot, channel) in behind.iter_mut().zip(self.outboxes.behind()) {
            *slot = channel;
            n += 1;
        }
        for &channel in &behind[..n] {
            if self.flush_client(channel) == Flush::Dead {
                if let Some(idx) = self.app_slot_of_channel(channel) {
                    self.note_app_channel_dead(idx);
                }
            }
        }
    }

    /// `channel` closes (its window closed, its client re-attached): what waited for it goes.
    pub(super) fn forget_client_channel(&mut self, channel: u32) {
        self.outboxes.forget(channel);
    }

    /// The loop's waitset: the writable signals are armed in it.
    pub(crate) fn set_client_waitset(&mut self, waitset: u32) {
        self.outboxes.set_waitset(waitset);
    }

    /// `(clients behind, the most frames one waits with, ORDERED frames refused)`.
    pub(crate) fn client_census(&self) -> (usize, usize, u32) {
        self.outboxes.census()
    }
}
