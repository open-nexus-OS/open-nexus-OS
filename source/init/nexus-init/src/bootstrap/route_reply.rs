// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Route-reply delivery for init's responder (RFC-0093 §1, TASK-0324 P3): ONE
//! nonce-echoed reply per ask, bounded-blocking and loud on failure, plus the resume path
//! that answers parked asks once their target's routes are re-provisioned.
//! OWNERS: @runtime
//! STATUS: Production (TASK-0324 P3)
//! API_STABILITY: Internal
//! TEST_COVERAGE: `route_park` unit tests + QEMU ladders (`init: route resumed`, restart lanes)

use crate::bootstrap::CtrlChannel;

/// Sends ONE nonce-echoed route reply (RFC-0093 §1).
///
/// Bounded-blocking, never silent: the control RSP queue is drained by the client, so a full
/// queue is transient — retry a bounded number of times with a yield between them. Init must
/// not block on a client, so the bound is attempts, not wall-clock. A reply that still cannot
/// be delivered is LOUD: the client would otherwise wait for an answer that never comes.
pub(crate) fn send_route_rsp(
    chan: &CtrlChannel,
    status: u8,
    send_slot: u32,
    recv_slot: u32,
    nonce: u32,
) {
    const ATTEMPTS: u32 = 8;
    let base = nexus_abi::routing::encode_route_rsp(status, send_slot, recv_slot);
    let mut rsp = [0u8; 17];
    rsp[..13].copy_from_slice(&base);
    rsp[13..17].copy_from_slice(&nonce.to_le_bytes());
    let rh = nexus_abi::MsgHeader::new(0, 0, 0, 0, rsp.len() as u32);
    for attempt in 0..ATTEMPTS {
        match nexus_abi::ipc_send_v1(
            chan.ctrl_rsp_parent_slot,
            &rh,
            &rsp,
            nexus_abi::IPC_SYS_NONBLOCK,
            0,
        ) {
            Ok(_) => return,
            Err(nexus_abi::IpcError::QueueFull) if attempt + 1 < ATTEMPTS => {
                let _ = nexus_abi::yield_();
            }
            Err(_) => break,
        }
    }
    crate::bootstrap::diag::emit_marker_atomic(
        &[b"init: FAIL route reply drop svc=", chan.svc_name.as_bytes()],
        None,
    );
}

/// Refuses a nonce-less route ask (RFC-0093 §1). The reply carries no nonce because the ask
/// carried none — a v1-shaped `STATUS_MALFORMED` frame is the only honest answer, and the
/// `!route-malformed` witness names the requester.
pub(crate) fn send_route_malformed(chan: &CtrlChannel) {
    let rsp = nexus_abi::routing::encode_route_rsp(nexus_abi::routing::STATUS_MALFORMED, 0, 0);
    let rh = nexus_abi::MsgHeader::new(0, 0, 0, 0, rsp.len() as u32);
    let _ = nexus_abi::ipc_send_v1(
        chan.ctrl_rsp_parent_slot,
        &rh,
        &rsp,
        nexus_abi::IPC_SYS_NONBLOCK,
        0,
    );
}

/// Answers every parked ask whose target became resolvable again (the supervisor
/// re-provisioned its routes). Exactly one reply per parked ask — RFC-0093 §1.
pub(crate) fn answer_parked_routes(
    park: &mut crate::route_park::RoutePark,
    ctrl_channels: &[CtrlChannel],
    route_table: &crate::route_table::RouteTable,
) {
    if park.is_empty() {
        return;
    }
    let mut resolved = [crate::route_park::ParkedRoute {
        chan: 0,
        target: crate::service_topology::ServiceId::Vfsd,
        nonce: 0,
    }; 8];
    let n = park.take_resolved(|target| !route_table.is_stale(target), &mut resolved);
    for ask in &resolved[..n] {
        let Some(chan) = ctrl_channels.get(ask.chan as usize) else {
            continue;
        };
        let (status, send_slot, recv_slot) = match route_table
            .lookup_by_name(chan.svc_name.as_bytes(), ask.target.name().as_bytes())
        {
            Ok(route) => (nexus_abi::routing::STATUS_OK, route.send.slot, route.recv.slot),
            Err(_) => (nexus_abi::routing::STATUS_NOT_FOUND, 0u32, 0u32),
        };
        crate::bootstrap::diag::emit_marker_atomic(
            &[
                b"init: route resumed svc=",
                chan.svc_name.as_bytes(),
                b" -> ",
                ask.target.name().as_bytes(),
            ],
            None,
        );
        send_route_rsp(chan, status, send_slot, recv_slot, ask.nonce);
    }
}
