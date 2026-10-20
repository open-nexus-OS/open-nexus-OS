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

use alloc::vec::Vec;

use super::endpoint::Endpoint;
use super::{EndpointId, IpcError, Router, WaiterId};
use crate::cap::{CapabilityKind, Rights};

impl Router {
    /// Capabilities to `id` that are IN FLIGHT — moved inside a message still queued on some
    /// live endpoint; the peer that dequeues them will hold them — counted in ONE pass as
    /// `(senders, receivers, any)`.
    #[must_use]
    pub fn in_flight_census(&self, id: EndpointId) -> (usize, usize, usize) {
        let (mut send, mut recv, mut any) = (0, 0, 0);
        for cap in self
            .endpoints
            .iter()
            .filter(|ep| ep.alive)
            .flat_map(|ep| ep.queue.iter())
            .filter_map(|msg| msg.moved_cap.as_ref())
        {
            if !matches!(cap.kind, CapabilityKind::Endpoint(ep) if ep == id) {
                continue;
            }
            any += 1;
            send += usize::from(cap.rights.contains(Rights::SEND));
            recv += usize::from(cap.rights.contains(Rights::RECV));
        }
        (send, recv, any)
    }

    /// Whether `id` is latched "no receiver can ever drain this" (a send returns
    /// `PeerClosed`). `false` for a missing or dead endpoint.
    #[must_use]
    pub fn receiver_gone(&self, id: EndpointId) -> bool {
        self.endpoints.get(id as usize).is_some_and(|ep| ep.alive && ep.receiver_gone)
    }

    /// Latches `id` as receiver-less and drains its send waiters so each re-runs its send and
    /// returns `PeerClosed` (the caller wakes them). No-op on a missing or dead endpoint.
    pub fn mark_receiver_gone(&mut self, id: EndpointId) -> Vec<WaiterId> {
        match self.endpoints.get_mut(id as usize) {
            Some(ep) if ep.alive => {
                ep.receiver_gone = true;
                ep.send_waiters.drain(..).collect()
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

    /// Every live endpoint owned by `owner`, appended to `out` (what its exit closes).
    pub fn owned_alive_ids(&self, owner: WaiterId, out: &mut Vec<EndpointId>) {
        for (id, ep) in self.endpoints.iter().enumerate() {
            if ep.alive && ep.owner == Some(owner) {
                out.push(id as EndpointId);
            }
        }
    }

    /// Every dead slot, appended to `out` — the candidates a create may reuse once the syscall
    /// layer has checked that no capability and no queued message names the id any more
    /// (the waitsets, timers, interrupt lines and exit bindings forgot it when it died).
    pub fn dead_endpoint_ids(&self, out: &mut Vec<EndpointId>) {
        for (id, ep) in self.endpoints.iter().enumerate() {
            if !ep.alive {
                out.push(id as EndpointId);
            }
        }
    }

    /// Gives dead slot `id` a fresh endpoint. The id of a slot something still names is never
    /// handed out again (the caller checked): a stale capability must keep naming nothing.
    pub fn revive_endpoint(
        &mut self,
        id: EndpointId,
        depth: usize,
        owner: Option<WaiterId>,
    ) -> Result<EndpointId, IpcError> {
        if self.endpoints.get(id as usize).is_none_or(|ep| ep.alive) {
            return Err(IpcError::NoSuchEndpoint);
        }
        if let Some(owner_pid) = owner {
            let count =
                self.endpoints.iter().filter(|ep| ep.alive && ep.owner == Some(owner_pid)).count();
            if count >= super::MAX_ENDPOINTS_PER_OWNER {
                return Err(IpcError::NoSpace);
            }
        }
        self.endpoints[id as usize] = Endpoint::with_depth(depth.clamp(1, 256), owner);
        Ok(id)
    }
}
