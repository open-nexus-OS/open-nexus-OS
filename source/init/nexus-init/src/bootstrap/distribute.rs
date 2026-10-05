// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Pre-grant server-pair distribution (task #123 hardening) —
//! every service that exposes a server gets its minted request/response
//! pair BEFORE the MMIO
//! grants, so no request can race the caps. Split from `wiring.rs` under
//! the structure ratchet; `distribute_server_pair_for` is the per-service
//! shape the TASK-0321 volume spawn pass applies to a service spawned
//! AFTER the bulk pass (so it ends up wired exactly like an embedded one).
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: QEMU marker ladder (`init: <svc> slots …` lines)
//! ADR: docs/adr/0060-verified-system-volume-bundlemgrd-verifier-init-spawner.md

use crate::bootstrap::endpoints::Endpoints;
use crate::bootstrap::CtrlChannel;

/// Distribute capabilities to every spawned service (the bespoke per-service
/// `match` + the declarative generic arm). Mutates each `CtrlChannel`'s slot
/// fields in place; the caller builds the route table from them afterward.
/// RFC-0069 phase semantics (task #123 fix): distribute each declared service's
/// PRE-MINTED server endpoint pair IMMEDIATELY after endpoint creation — before
/// the policy-gated MMIO grant phase. The services' declared server slots
/// then hold the pair no matter how long policyd takes to answer grants;
/// previously a slow policyd delayed `wire_services` past the services'
/// route-probe fallback, their first recv hit an EMPTY slot, and the whole
/// early fleet died (init then aborted wiring caps into dead PIDs). Silent and
/// best-effort; `wire_services` keeps the announce prints + fold tally at the
/// historical log position and skips the pair once set.
pub(crate) fn distribute_server_pairs(ctrls: &mut [CtrlChannel], eps: &Endpoints) {
    for chan in ctrls.iter_mut() {
        distribute_server_pair_for(chan, eps);
    }
}

/// One service's share of the pre-grant distribution — also the shape the
/// volume spawn pass (TASK-0321) applies to a service spawned AFTER the
/// bulk pass, so it ends up wired exactly like an embedded one.
pub(crate) fn distribute_server_pair_for(chan: &mut CtrlChannel, eps: &Endpoints) {
    {
        let name = chan.svc_name;
        // Authority = the minted-pair table itself (None for the priority-wired drivers).
        let Some(id) = crate::service_topology::ServiceId::from_name(name.as_bytes()) else {
            return;
        };
        if chan.send(id).is_some() && chan.recv(id).is_some() {
            return;
        }
        // TASK-0054C P2-b, TASK-0328 U1: the declared wait endpoints (pacing timer, device
        // watchdog, interrupt notify), minted and pinned HERE — before the service is resumed —
        // for every service that is not on the core plane (`core_plane::transfer_server_pair`
        // pins those, earlier still).
        crate::bootstrap::declared_routes::pin_declared_waits(chan.pid, id);
        // No minted pair: nothing to distribute. For a declared server that is a bootstrap defect,
        // which `wire_services` reports; the fresh-endpoint fallback that used to hide it would
        // have orphaned every client of the minted pair (TASK-0324 P4f-6).
        let Some((req, rsp)) = eps.server_pair(id) else {
            return;
        };
        // TASK-0324 P4: the server pair is pinned into its DECLARED slots — never an
        // order-based transfer (the last one, for netstackd and metricsd, died in P4f-4), and
        // never a silent fallback: a failed pin is announced by `pin` and leaves the service
        // unwired, a dead route with a witness instead of a service listening on a slot nobody
        // knows.
        if let Some(slots) =
            crate::bootstrap::declared_slots::pin_server_pair(chan.pid, id, req, rsp)
        {
            chan.set_send(id, slots.send);
            chan.set_recv(id, slots.recv);
        }
        crate::bootstrap::blk_plane::wire_blk_plane_for(chan, eps);
    }
}
