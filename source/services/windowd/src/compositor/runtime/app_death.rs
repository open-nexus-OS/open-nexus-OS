// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: A dead client is found by name, never waited on (TASK-0333 6a). windowd pushes
//! into an app's dedicated event channel; when the app's process is gone, no RECV capability
//! to that channel exists any more and the kernel answers the push with `PeerClosed` (the
//! sender's mirror of RFC-0079's EOF) — or `NoSuchEndpoint` once the endpoint itself died
//! with its last capability. Either is the scene's signal: the window closes exactly as the
//! close button closes it (`close_app_window`: band, surface capability, event channel, slot),
//! and the console names it. Before this a push into a crashed app's queue succeeded until the
//! queue was full, and the next critical push parked the compositor on a peer that was gone.
//! The slow-but-alive client (a full queue) is unchanged here; its contract is 6d's.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `windowd: app died (slot=` on the board's crash path; the kernel half is
//!   `KSELFTEST: ipc peer closed send ok` every boot

use super::DisplayServerRuntime;
#[cfg(nexus_env = "os")]
use crate::client_surface::Delivery;

/// What one push into a client's channel came to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ClientSend {
    /// Delivered.
    Sent,
    /// Not delivered: the queue is full and the delivery class does not park (the client is
    /// behind; the next frame carries the newer truth), or the park ended without delivery.
    Behind,
    /// The peer is gone: no receiver exists any more, or the endpoint died.
    Dead,
}

/// One push, classified. Coalescing deliveries try once; the others park as send-waiters
/// until the client drains (RFC-0093 §7) — a dead client ends that park with an error, which
/// is the `Dead` verdict, never a retry.
#[cfg(nexus_env = "os")]
pub(super) fn send_client_frame_outcome(slot: u32, frame: &[u8], delivery: Delivery) -> ClientSend {
    let hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, frame.len() as u32);
    let flags = if delivery.parks() { 0 } else { nexus_abi::IPC_SYS_NONBLOCK };
    match nexus_abi::ipc_send_v1(slot, &hdr, frame, flags, 0) {
        Ok(_) => ClientSend::Sent,
        Err(nexus_abi::IpcError::PeerClosed) | Err(nexus_abi::IpcError::NoSuchEndpoint) => {
            ClientSend::Dead
        }
        Err(_) => ClientSend::Behind,
    }
}

impl DisplayServerRuntime {
    /// The app behind window slot `idx` is gone: close its window like the close button does
    /// and say so. Idempotent — a slot without a surface has nothing left to close.
    pub(crate) fn note_app_channel_dead(&mut self, idx: usize) {
        if self.apps[idx].surface_id.is_none() {
            return;
        }
        let _ = nexus_abi::debug_println(&alloc::format!(
            "windowd: app died (slot={idx} owner={:#x})",
            self.apps[idx].owner_sid
        ));
        self.close_app_window(idx);
    }

    /// The window slot an event channel belongs to (the presentation pump knows channels).
    #[cfg(nexus_env = "os")]
    pub(super) fn app_slot_of_channel(&self, slot: u32) -> Option<usize> {
        self.apps.iter().position(|a| a.event_channel == Some(slot))
    }
}
