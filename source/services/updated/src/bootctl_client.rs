// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg(all(nexus_env = "os", feature = "os-lite"))]
#![forbid(unsafe_code)]

//! CONTEXT: updated's bootctld client (TASK-0050 PR-2, ADR-0055). updated
//! no longer owns the boot record — every slot mutation delegates to the
//! single boot-state authority over the bootctld wire (`B`,`T`; ops mirror
//! the old in-process semantics 1:1). Route is resolved once via the init
//! responder and cached; replies ride updated's OWN CAP_MOVE inbox
//! (deterministic slots 0x0A/0x0B) — never a shared response queue.
//! Foreign frames on the inbox (statefs/logd acks) are skipped by magic +
//! op echo; updated keeps at most ONE bootctld call in flight, so no
//! nonce is needed on this wire yet.
//! OWNERS: @services-team @reliability
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: QEMU OTA ladder (markers unchanged, relocated
//!   authority): `SELFTEST: ota stage|switch|health|rollback ok`,
//!   `SELFTEST: bootctl persist ok`.
//! ADR: docs/adr/0055-bootctld-single-boot-state-authority.md

use bootctld::wire;

/// updated's CAP_MOVE reply inbox as declared (TASK-0324 P4f-3).
const REPLY_RECV_SLOT: u32 = nexus_service_topology::slots::updated::REPLY.recv;
const REPLY_SEND_SLOT: u32 = nexus_service_topology::slots::updated::REPLY.send;

/// A decoded bootctld reply: wire status + up to 20 payload bytes
/// (GET_STATUS grew additive tails: TASK-0036-B projection, TASK-0179
/// floor).
pub(crate) struct BootctlReply {
    pub status: u8,
    pub payload: [u8; 20],
    pub payload_len: usize,
}

/// One bounded request/reply exchange with bootctld. `None` = transport
/// trouble (route/send/recv) — the caller maps it to its FAILED audit.
pub(crate) fn call(op: u8, arg: Option<u8>) -> Option<BootctlReply> {
    match arg {
        Some(byte) => call_with_args(op, &[byte]),
        None => call_with_args(op, &[]),
    }
}

/// Like [`call`], with an arbitrary bounded arg tail (OP_STAGE carries the
/// staged rollback index as 4 LE bytes — TASK-0179 §10 commit rung).
pub(crate) fn call_with_args(op: u8, args: &[u8]) -> Option<BootctlReply> {
    let send_slot = cached_send_slot()?;
    let mut frame = [0u8; 12];
    if args.len() > 8 {
        return None;
    }
    frame[0] = wire::MAGIC0;
    frame[1] = wire::MAGIC1;
    frame[2] = wire::VERSION;
    frame[3] = op;
    frame[4..4 + args.len()].copy_from_slice(args);
    let len = 4 + args.len();
    // ONE exchange; the inbox is shared with updated's other legs, so the answer is the frame
    // carrying bootctld's magic and OUR op echo — everything else queued belongs to another leg
    // (TASK-0054C P2-d).
    let mut buf = [0u8; 32];
    nexus_ipc::exchange::call_matching(
        send_slot,
        nexus_ipc::SlotPair::new(REPLY_SEND_SLOT, REPLY_RECV_SLOT),
        &frame[..len],
        &mut buf,
        |rsp| {
            if rsp.len() < 7
                || rsp[0] != wire::MAGIC0
                || rsp[1] != wire::MAGIC1
                || rsp[2] != wire::VERSION
                || rsp[3] != (op | 0x80)
            {
                return None;
            }
            let payload_len = core::cmp::min(u16::from_le_bytes([rsp[5], rsp[6]]) as usize, 20);
            let mut payload = [0u8; 20];
            let avail = core::cmp::min(payload_len, rsp.len() - 7);
            payload[..avail].copy_from_slice(&rsp[7..7 + avail]);
            Some(BootctlReply { status: rsp[4], payload, payload_len: avail })
        },
    )
    .ok()
}

/// bootctld's request endpoint: the DECLARED slot, pinned by init before this task runs —
/// no runtime route ask (TASK-0324 P7-d).
fn cached_send_slot() -> Option<u32> {
    Some(nexus_service_topology::slots::updated::BOOTCTLD.send)
}
