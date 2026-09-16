// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: metricsd retention statefs I/O (split from `os_lite.rs` under
//! the structure ratchet, TASK-0049C). Bulk PUT/DEL are fire-and-forget
//! WITH a reply consumer: CAP_MOVE onto metricsd's OWN inbox, drained
//! nonblocking before each send — the previous consumer-less sends rotted
//! replies on statefsd's SHARED response queue until the store wedged deaf
//! (the 0049B wedge class).
//!
//! TASK-0054C P2-c: this is the ONE site the "a reply inbox sees only awaited
//! replies" invariant does not cover, and neither of the two obvious fixes is
//! available yet. Sending WITHOUT a reply cap is out: statefsd always answers,
//! and a cap-less sender is answered on statefsd's shared response queue with a
//! BLOCKING send — the 0049B wedge, in the store everyone depends on. AWAITING
//! the status is out too, and measurably so: statefsd's quota gate runs inside
//! its PUT handler (`abi_seam_os::put_gates` -> `QuotaState::admit_put`), a deny
//! flushes `quota_denies_total` through `nexus_metrics::DenyCounter`, and that
//! flush WAITS for metricsd's answer — so a metricsd that waits here deadlocks
//! against a statefsd that is waiting on metricsd. The real fix is a one-way
//! write op in the statefs protocol (a PUT/DEL statefsd does not answer), which
//! is a wire change and belongs to its own package (TASK-0054C P2-d).
//! OWNERS: @runtime @observability
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: QEMU retention markers (`metricsd: retention wal
//!   active|verified`).
//! ADR: docs/rfcs/RFC-0087-reliability-failure-model-v1.md

use nexus_abi::yield_;
use nexus_ipc::KernelClient;

use statefs::protocol as statefs_proto;

use nexus_service_topology::slots::metricsd::REPLY;

// TASK-0049C hardening: the retention bulk path used to send PUT/DEL with
// NO reply consumer — every reply rotted on statefsd's SHARED response
// queue (depth 8) until the store went deaf for everyone (the 0049B wedge
// class). Fire-and-forget now means: CAP_MOVE the reply to metricsd's OWN
// inbox and drain it nonblocking before each send — abandoned replies cost
// metricsd its own inbox, never the shared queue.
pub(crate) fn statefs_fire_and_forget(client: &KernelClient, frame: &[u8]) -> bool {
    drain_own_reply_inbox();
    let Ok(reply_send_clone) = nexus_abi::cap_clone(REPLY.send) else {
        return false;
    };
    let hdr = nexus_abi::MsgHeader::new(
        reply_send_clone,
        0,
        0,
        nexus_abi::ipc_hdr::CAP_MOVE,
        frame.len() as u32,
    );
    let (send_slot, _) = client.slots();
    match nexus_abi::ipc_send_v1(send_slot, &hdr, frame, nexus_abi::IPC_SYS_NONBLOCK, 0) {
        Ok(_) => true,
        Err(_) => {
            let _ = nexus_abi::cap_close(reply_send_clone);
            false
        }
    }
}

/// Discard whatever replies sit in metricsd's own inbox (bounded, nonblock).
pub(crate) fn drain_own_reply_inbox() {
    for _ in 0..8 {
        let mut rh = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
        let mut buf = [0u8; 96];
        if nexus_abi::ipc_recv_v1(
            REPLY.recv,
            &mut rh,
            &mut buf,
            nexus_abi::IPC_SYS_NONBLOCK | nexus_abi::IPC_SYS_TRUNCATE,
            0,
        )
        .is_err()
        {
            break;
        }
    }
}

pub(crate) fn statefs_put_nonblocking(client: &KernelClient, key: &str, value: &[u8]) -> bool {
    let Ok(frame) = statefs_proto::encode_put_request(key, value) else {
        return false;
    };
    statefs_fire_and_forget(client, &frame)
}

pub(crate) fn statefs_delete_nonblocking(client: &KernelClient, key: &str) -> bool {
    let Ok(frame) = statefs_proto::encode_key_only_request(statefs_proto::OP_DEL, key) else {
        return false;
    };
    statefs_fire_and_forget(client, &frame)
}

pub(crate) fn put_with_retries(
    client: &KernelClient,
    key: &str,
    value: &[u8],
    retries: u32,
) -> bool {
    for _ in 0..retries {
        if statefs_put_nonblocking(client, key, value) {
            return true;
        }
        let _ = yield_();
    }
    false
}
