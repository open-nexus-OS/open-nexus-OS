// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The router's RFC-0079 last-sender-EOF accessors — the `had_sender` latch, the
//! endpoint owner (TASK-0324 P7-b: an owner is never its own peer) and the recv-waiter drain
//! the EOF scan wakes. Split out of `ipc/mod.rs` (structure-gate); the decision itself is the
//! host-tested predicate in `crate::ipc_eof`, the per-task scan lives in
//! `syscall::api::eof_scan`.
//! OWNERS: @kernel-ipc-team
//! STATUS: Functional
//! API_STABILITY: Unstable

use alloc::vec::Vec;

use super::{EndpointId, Router, WaiterId};

impl Router {
    /// RFC-0079 / TASK-0324 P7-b: SEND caps to `id` that are IN FLIGHT — moved inside a
    /// message still queued on some endpoint, held by no task's table yet. The peer that will
    /// dequeue them is a sender the last-peer scan must count, or a client whose request
    /// (with its reply cap) has not been received yet would EOF against itself: its own SEND
    /// base is excluded as the channel owner, and nothing else holds a cap at that instant.
    #[must_use]
    pub fn in_flight_send_cap_count(&self, id: EndpointId) -> usize {
        self.endpoints
            .iter()
            .filter(|ep| ep.alive)
            .flat_map(|ep| ep.queue.iter())
            .filter_map(|msg| msg.moved_cap.as_ref())
            .filter(|cap| {
                matches!(cap.kind, crate::cap::CapabilityKind::Endpoint(ep) if ep == id)
                    && cap.rights.contains(crate::cap::Rights::SEND)
            })
            .count()
    }

    /// RFC-0079: whether endpoint `id` has EVER had a sender (the monotonic
    /// latch). `false` for a missing/dead endpoint. The EOF decision requires
    /// this true, so an endpoint that never had a sender never wrongly EOFs.
    #[must_use]
    pub fn endpoint_had_sender(&self, id: EndpointId) -> bool {
        self.endpoints.get(id as usize).is_some_and(|ep| ep.had_sender)
    }

    /// The task the endpoint was created FOR (the factory's `owner`), if recorded. The
    /// last-sender scan excludes it: an owner is never its own peer (TASK-0324 P7-b).
    #[must_use]
    pub fn endpoint_owner(&self, id: EndpointId) -> Option<WaiterId> {
        self.endpoints.get(id as usize).and_then(|ep| ep.owner)
    }

    /// RFC-0079: latches endpoint `id` as having had a sender (called when the
    /// recv-block scan observes a live SEND cap, complementing the send path).
    pub fn mark_endpoint_had_sender(&mut self, id: EndpointId) {
        if let Some(ep) = self.endpoints.get_mut(id as usize) {
            ep.had_sender = true;
        }
    }

    /// TASK-0324 P7-d: latches "the last peer is gone" on `id` (set by the scan that decided
    /// EOF) so `waitset_wait` reports the member ready; see `Endpoint::eof_pending`.
    pub fn set_eof_pending(&mut self, id: EndpointId) {
        if let Some(ep) = self.endpoints.get_mut(id as usize) {
            ep.eof_pending = true;
        }
    }

    /// Clears the EOF latch of `id` (a receiver observed the endpoint, or a sender wrote).
    pub fn clear_eof_pending(&mut self, id: EndpointId) {
        if let Some(ep) = self.endpoints.get_mut(id as usize) {
            ep.eof_pending = false;
        }
    }

    /// Whether `id` carries the EOF latch (a waitset readiness input).
    #[must_use]
    pub fn eof_pending(&self, id: EndpointId) -> bool {
        self.endpoints.get(id as usize).is_some_and(|ep| ep.alive && ep.eof_pending)
    }

    /// RFC-0079: drains endpoint `id`'s recv-waiters so they re-run recv (used
    /// when the last SEND cap closed — a blocked EOF-opted receiver re-scans
    /// and returns `PeerClosed`). Empty for a missing/dead endpoint.
    pub fn drain_recv_waiters(&mut self, id: EndpointId) -> Vec<WaiterId> {
        match self.endpoints.get_mut(id as usize) {
            Some(ep) if ep.alive => ep.recv_waiters.drain(..).collect(),
            _ => Vec::new(),
        }
    }
}
