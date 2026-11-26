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
use crate::cap::{Held, HeldCounts, Rights};

impl Router {
    /// The header of endpoint `id`: the capabilities held to it, in a table or in a message
    /// (RFC-0102). A dead slot keeps its header — a stale capability still counts there.
    #[must_use]
    pub fn held_counts(&self, id: EndpointId) -> Option<Arc<HeldCounts>> {
        self.ep(id).map(|ep| ep.counts.clone())
    }

    /// A new capability to the live endpoint `id` with `rights` — the endpoint's own door
    /// (its creation, the kernel's wiring of a task): a capability to an endpoint is made
    /// here or from another one (`Held::dup`), never from the id alone.
    pub fn mint(&self, id: EndpointId, rights: Rights) -> Result<Held, IpcError> {
        match self.ep(id) {
            Some(ep) if ep.alive => Ok(ep.counts.mint(rights)),
            _ => Err(IpcError::NoSuchEndpoint),
        }
    }

    /// One capability that was in flight in an endpoint closed since (RFC-0112, amendment
    /// 2026-11-02), dropped here — counted off — and handed to the caller with its object's
    /// header, who judges the object like after a `cap_close`
    /// (`syscall::api::eof_scan::release_in_flight`). `None` once all are out.
    pub fn take_in_flight_dropped(&self) -> Option<crate::cap::Dropped> {
        // The lock is released before the capability is dropped: counting it off may close
        // another endpoint. Its header stays with the caller for the judgement.
        let held = self.in_flight_dropped.lock().pop();
        held.map(Held::dropped)
    }

    /// Creates a new kernel endpoint and returns its identifier.
    pub fn create_endpoint(
        &self,
        depth: usize,
        owner: Option<WaiterId>,
    ) -> Result<EndpointId, IpcError> {
        // The registry lock: one creation or reuse at a time (ADR-0102).
        let mut owners = self.owners.lock();
        if owner.is_some_and(|pid| self.owned_alive(pid) >= super::MAX_ENDPOINTS_PER_OWNER) {
            return Err(IpcError::NoSpace);
        }
        // The id is the next slot's; everything the endpoint needs is made before the table
        // grows, so a refusal leaves no slot behind.
        let id = self.slot_count() as EndpointId;
        let endpoint = Self::fresh(&mut owners, id, depth, owner)?;
        // ADR-0093: the table grows by a block, above the kernel heap's reserve — or `ENOSPC`.
        let grown = self.grow().ok_or(IpcError::NoSpace)?;
        self.install(grown, endpoint);
        Ok(grown)
    }

    /// A fresh endpoint for slot `id`, with a header of its own and its owner's byte counter.
    fn fresh(
        owners: &mut super::table::Owners,
        id: EndpointId,
        depth: usize,
        owner: Option<WaiterId>,
    ) -> Result<Endpoint, IpcError> {
        let counts = HeldCounts::endpoint(id, owner).ok_or(IpcError::NoSpace)?;
        let owner_bytes = Self::owner_counter(owners, owner).map_err(|()| IpcError::NoSpace)?;
        let mut endpoint = Endpoint::with_depth(depth.clamp(1, 256), owner, counts);
        endpoint.owner_bytes = owner_bytes;
        Ok(endpoint)
    }

    /// Whether `id` is latched "no receiver can ever drain this" (a send returns
    /// `PeerClosed`). `false` for a missing or dead endpoint.
    #[must_use]
    pub fn receiver_gone(&self, id: EndpointId) -> bool {
        self.ep(id).is_some_and(|ep| ep.alive && ep.receiver_gone)
    }

    /// Latches `id` as receiver-less and drains its send waiters so each re-runs its send and
    /// returns `PeerClosed` — and its writable waiters, whose waitset now reports the member
    /// ready (the caller wakes them). No-op on a missing or dead endpoint.
    pub fn mark_receiver_gone(&self, id: EndpointId) -> Vec<WaiterId> {
        match self.ep(id) {
            Some(mut ep) if ep.alive => {
                let ep = &mut *ep;
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
        let mut live = 0;
        self.each(|_, ep| live += usize::from(ep.alive));
        live
    }

    /// Live endpoints `owner` owns (the per-owner valve's count).
    pub(super) fn owned_alive(&self, owner: WaiterId) -> usize {
        let mut owned = 0;
        self.each(|_, ep| owned += usize::from(ep.alive && ep.owner == Some(owner)));
        owned
    }

    /// Every live endpoint owned by `owner`, appended to `out` (what its exit closes).
    pub fn owned_alive_ids(&self, owner: WaiterId, out: &mut Vec<EndpointId>) {
        self.each(|id, ep| {
            if ep.alive && ep.owner == Some(owner) {
                out.push(id);
            }
        });
    }

    /// A dead slot no capability is held to any more, in a table or in a message — one a
    /// create may give a fresh endpoint (the waitsets, timers, interrupt lines and exit
    /// bindings forgot the id when it died).
    #[must_use]
    pub fn reusable_dead_endpoint(&self) -> Option<EndpointId> {
        (0..self.slot_count() as EndpointId)
            .find(|&id| self.ep(id).is_some_and(|ep| Self::forgotten(&ep)))
    }

    /// Whether a slot's id may go to a new endpoint: dead, named by no capability, and its
    /// header held by nobody but the slot — a syscall working on it off the kernel lock and a
    /// caller waiting on it hold the header (`cap::PinnedEndpoint`, TASK-0330 P2). A pin is
    /// taken from a capability in a table, under that table's lock: once none names the slot,
    /// no new pin appears.
    fn forgotten(ep: &Endpoint) -> bool {
        !ep.alive && ep.counts.any() == 0 && Arc::strong_count(&ep.counts) == 1
    }

    /// Gives dead slot `id` a fresh endpoint with a header of its own. The id of a slot a
    /// capability is still held to is never handed out again: a stale capability must keep
    /// holding nothing.
    pub fn revive_endpoint(
        &self,
        id: EndpointId,
        depth: usize,
        owner: Option<WaiterId>,
    ) -> Result<EndpointId, IpcError> {
        let mut owners = self.owners.lock();
        if self.ep(id).is_none_or(|ep| !Self::forgotten(&ep)) {
            return Err(IpcError::NoSuchEndpoint);
        }
        if owner.is_some_and(|pid| self.owned_alive(pid) >= super::MAX_ENDPOINTS_PER_OWNER) {
            return Err(IpcError::NoSpace);
        }
        let endpoint = Self::fresh(&mut owners, id, depth, owner)?;
        self.install(id, endpoint);
        Ok(id)
    }
}
