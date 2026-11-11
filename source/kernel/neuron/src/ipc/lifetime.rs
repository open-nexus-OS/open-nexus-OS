// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The router's side of "an endpoint dies with its last capability" (RFC-0100 §1,
//! TASK-0333 6a). Before this an endpoint lived until its OWNER exited — the pairs init mints
//! for every app launch (events, timer) were owned by execd, which closed its own halves at
//! once, so each launch left two endpoints that nothing could ever close, and ids were never
//! reused: the table (384) and the per-owner quota (96) were a launch counter. Now the scan in
//! `syscall::api::ipc_reclaim` destroys an endpoint nobody names and nothing carries, and a
//! create prefers a dead slot that no capability names any more. The receiver's mirror of
//! RFC-0079 lives here too: the latch that turns a send into `PeerClosed` once no RECV
//! capability exists anywhere.
//! OWNERS: @kernel-ipc-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `KSELFTEST: task exit releases ok`, `KSELFTEST: ipc peer closed send ok`;
//!   the rules themselves are host-tested in `crate::ipc_eof`

use alloc::sync::Arc;
use alloc::vec::Vec;

use super::endpoint::Endpoint;
use super::{EndpointId, IpcError, Router, WaiterId};
use crate::cap::{Capability, Held, HeldCounts, Rights};

impl Router {
    /// The header of endpoint `id`: the capabilities held to it, in a table or in a message
    /// (RFC-0102). A dead slot keeps its header — a stale capability still counts there.
    #[must_use]
    pub fn held_counts(&self, id: EndpointId) -> Option<&Arc<HeldCounts>> {
        self.endpoints.get(id as usize).map(|ep| &ep.counts)
    }

    /// A new capability to the live endpoint `id` with `rights` — the endpoint's own door
    /// (its creation, the kernel's wiring of a task): a capability to an endpoint is made
    /// here or from another one (`Held::dup`), never from the id alone.
    pub fn mint(&self, id: EndpointId, rights: Rights) -> Result<Held, IpcError> {
        match self.endpoints.get(id as usize) {
            Some(ep) if ep.alive => Ok(ep.counts.mint(rights)),
            _ => Err(IpcError::NoSuchEndpoint),
        }
    }

    /// One capability that was in flight in an endpoint closed since (RFC-0112, amendment
    /// 2026-11-02), dropped here — counted off — and described to the caller, who judges its
    /// object like after a `cap_close` (`syscall::api::eof_scan::release_in_flight`). `None`
    /// once all are out.
    pub fn take_in_flight_dropped(&mut self) -> Option<Capability> {
        self.in_flight_dropped.pop().map(|held| held.cap())
    }

    /// Creates a new kernel endpoint and returns its identifier.
    pub fn create_endpoint(
        &mut self,
        depth: usize,
        owner: Option<WaiterId>,
    ) -> Result<EndpointId, IpcError> {
        let depth = depth.clamp(1, 256);
        if owner.is_some_and(|pid| self.owned_alive(pid) >= super::MAX_ENDPOINTS_PER_OWNER) {
            return Err(IpcError::NoSpace);
        }
        // ADR-0093: a full table doubles, above the kernel heap's reserve — or `ENOSPC`.
        let more = self.endpoints.capacity().max(8);
        if self.endpoints.len() == self.endpoints.capacity()
            && (!crate::kheap::admits(more * core::mem::size_of::<Endpoint>())
                || self.endpoints.try_reserve(more).is_err())
        {
            return Err(IpcError::NoSpace);
        }
        let id = self.endpoints.len() as EndpointId;
        let counts = HeldCounts::endpoint(id, owner).ok_or(IpcError::NoSpace)?;
        self.endpoints.push(Endpoint::with_depth(depth, owner, counts));
        Ok(id)
    }

    /// Whether `id` is latched "no receiver can ever drain this" (a send returns
    /// `PeerClosed`). `false` for a missing or dead endpoint.
    #[must_use]
    pub fn receiver_gone(&self, id: EndpointId) -> bool {
        self.endpoints.get(id as usize).is_some_and(|ep| ep.alive && ep.receiver_gone)
    }

    /// Latches `id` as receiver-less and drains its send waiters so each re-runs its send and
    /// returns `PeerClosed` — and its writable waiters, whose waitset now reports the member
    /// ready (the caller wakes them). No-op on a missing or dead endpoint.
    pub fn mark_receiver_gone(&mut self, id: EndpointId) -> Vec<WaiterId> {
        match self.endpoints.get_mut(id as usize) {
            Some(ep) if ep.alive => {
                ep.receiver_gone = true;
                ep.send_waiters.drain(..).chain(ep.writable_waiters.drain(..)).collect()
            }
            _ => Vec::new(),
        }
    }

    /// Live endpoints in the table (the record `KSELFTEST: vm reclaim (… endpoints=)` and the
    /// memory record read).
    #[must_use]
    pub fn live_endpoint_count(&self) -> usize {
        self.endpoints.iter().filter(|ep| ep.alive).count()
    }

    /// Live endpoints `owner` owns (the per-owner valve's count).
    pub(super) fn owned_alive(&self, owner: WaiterId) -> usize {
        self.endpoints.iter().filter(|ep| ep.alive && ep.owner == Some(owner)).count()
    }

    /// Every live endpoint owned by `owner`, appended to `out` (what its exit closes).
    pub fn owned_alive_ids(&self, owner: WaiterId, out: &mut Vec<EndpointId>) {
        for (id, ep) in self.endpoints.iter().enumerate() {
            if ep.alive && ep.owner == Some(owner) {
                out.push(id as EndpointId);
            }
        }
    }

    /// A dead slot no capability is held to any more, in a table or in a message — one a
    /// create may give a fresh endpoint (the waitsets, timers, interrupt lines and exit
    /// bindings forgot the id when it died).
    #[must_use]
    pub fn reusable_dead_endpoint(&self) -> Option<EndpointId> {
        self.endpoints
            .iter()
            .position(|ep| !ep.alive && ep.counts.any() == 0)
            .map(|id| id as EndpointId)
    }

    /// Gives dead slot `id` a fresh endpoint with a header of its own. The id of a slot a
    /// capability is still held to is never handed out again: a stale capability must keep
    /// holding nothing.
    pub fn revive_endpoint(
        &mut self,
        id: EndpointId,
        depth: usize,
        owner: Option<WaiterId>,
    ) -> Result<EndpointId, IpcError> {
        if self.endpoints.get(id as usize).is_none_or(|ep| ep.alive || ep.counts.any() > 0) {
            return Err(IpcError::NoSuchEndpoint);
        }
        if owner.is_some_and(|pid| self.owned_alive(pid) >= super::MAX_ENDPOINTS_PER_OWNER) {
            return Err(IpcError::NoSpace);
        }
        let counts = HeldCounts::endpoint(id, owner).ok_or(IpcError::NoSpace)?;
        self.endpoints[id as usize] = Endpoint::with_depth(depth.clamp(1, 256), owner, counts);
        Ok(id)
    }
}
