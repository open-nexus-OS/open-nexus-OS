// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Pre-grant server-pair distribution (task #123 hardening) —
//! every service that exposes a server gets its minted request/response
//! pair (or a fresh one for spec-declared plain servers) BEFORE the MMIO
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
use crate::bootstrap::wiring::is_bespoke_wired;
use crate::bootstrap::CtrlChannel;
use crate::os_payload::ENDPOINT_FACTORY_CAP_SLOT;
use nexus_abi::Rights;

/// Distribute capabilities to every spawned service (the bespoke per-service
/// `match` + the declarative generic arm). Mutates each `CtrlChannel`'s slot
/// fields in place; the caller builds the route table from them afterward.
/// RFC-0069 phase semantics (task #123 fix): distribute each declared service's
/// PRE-MINTED server endpoint pair IMMEDIATELY after endpoint creation — before
/// the policy-gated MMIO grant phase. The services' deterministic fallback
/// slots (3/4) then exist no matter how long policyd takes to answer grants;
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
        // Authority = the minted-pair table itself (covers declared AND
        // still-bespoke services; returns None for drivers/dsoftbusd).
        let Some(id) = crate::service_topology::ServiceId::from_name(name.as_bytes()) else {
            return;
        };
        if chan.send(id).is_some() && chan.recv(id).is_some() {
            return;
        }
        let Some((req, rsp)) = eps.server_pair(id) else {
            // No minted pair: spec-declared plain servers (abilitymgr,
            // sessiond) are provisioned fresh HERE — same pre-grant
            // hardening. Silent; `wire_services` prints the slots at the
            // historical log position from the recorded values.
            if crate::service_topology::exposes_server(name.as_bytes()) && !is_bespoke_wired(name) {
                if let Ok(ep) =
                    nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, chan.pid, 8)
                {
                    let recv = nexus_abi::cap_transfer(chan.pid, ep, Rights::RECV);
                    let send = nexus_abi::cap_transfer(chan.pid, ep, Rights::SEND);
                    let _ = nexus_abi::cap_close(ep);
                    if let (Ok(recv_slot), Ok(send_slot)) = (recv, send) {
                        chan.set_send(id, send_slot);
                        chan.set_recv(id, recv_slot);
                    }
                }
            }
            return;
        };
        let recv = nexus_abi::cap_transfer(chan.pid, req, Rights::RECV);
        let send = nexus_abi::cap_transfer(chan.pid, rsp, Rights::SEND);
        if let (Ok(recv_slot), Ok(send_slot)) = (recv, send) {
            chan.set_send(id, send_slot);
            chan.set_recv(id, recv_slot);
        }
        crate::bootstrap::blk_plane::wire_blk_plane_for(chan, eps);
    }
}
