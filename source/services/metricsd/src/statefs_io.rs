// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: metricsd retention statefs I/O (split from `os_lite.rs` under
//! the structure ratchet, TASK-0049C). Bulk PUT/DEL are fire-and-forget and
//! move NO capability: statefsd answers exactly the senders that moved a reply
//! cap (TASK-0054C P2-f), so no ack exists for these writes and there is
//! nothing to rot on anyone's inbox.
//!
//! The history is the reason this file still has a header. TASK-0049C found the
//! writes sending with no reply consumer at all, which rotted replies on
//! statefsd's SHARED response queue until the store went deaf for everyone (the
//! 0049B wedge class), and answered it by moving a reply cap onto metricsd's own
//! inbox and draining that inbox blind before each send. That traded one rot for
//! another: the abandoned acks sat on the inbox metricsd's own statefs client
//! reads, and the drain ate its replies. TASK-0054C P2-c named it the ONE site
//! its "a reply inbox sees only awaited replies" invariant could not cover,
//! because both exits were shut — a cap-less write was answered on the shared
//! queue, and awaiting the status risks a wait cycle through statefsd's quota
//! gate. P2-f opened the first exit by changing the SERVER: with no cap moved,
//! statefsd records the write and says nothing.
//! OWNERS: @runtime @observability
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: QEMU retention markers (`metricsd: retention wal
//!   active|verified`).
//! ADR: docs/rfcs/RFC-0087-reliability-failure-model-v1.md

use nexus_abi::yield_;
use nexus_ipc::KernelClient;

use statefs::protocol as statefs_proto;

/// One retention write: no reply capability moves with it, so statefsd journals it and never
/// answers. Non-blocking by contract — a full queue drops the write and the caller retries.
pub(crate) fn statefs_fire_and_forget(client: &KernelClient, frame: &[u8]) -> bool {
    nexus_ipc::exchange::send_nonblocking(client.slots().0, frame).is_ok()
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
