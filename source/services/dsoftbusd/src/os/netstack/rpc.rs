// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The ONE nonce-correlated netstack RPC: a CAP_MOVE request on dsoftbusd's
//! declared reply inbox, waited out until the frame carrying this op and nonce arrives.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: QEMU — every dsoftbusd netstack leg in the ladder runs through here.
//! ADR: docs/adr/0005-dsoftbus-architecture.md

use super::validate::response_matches;
use nexus_ipc::KernelClient;
use nexus_service_topology::SlotPair;

#[inline]
pub(crate) fn next_nonce(n: &mut u64) -> u64 {
    let out = *n;
    *n = n.wrapping_add(1);
    out
}

/// The ONE netstack RPC (TASK-0054C P2-c). dsoftbusd's reply inbox is structurally shared —
/// samgrd, bundlemgrd and logd all answer into it — so the answer is the frame that carries
/// OUR op AND OUR nonce; everything queued ahead of it belongs to another leg and is dropped.
///
/// What went with the two bodies this replaces: a 500 ms `nsec()` deadline in one, a
/// 200 000-iteration spin in the other, a `Wait::NonBlocking` request send that turned a
/// momentarily full netstackd queue into a silently lost RPC, a `cap_close` of a cap CAP_MOVE
/// had already consumed, and the `ReplyBuffer` that parked the replies those budgets
/// abandoned. Nothing is abandoned any more, so nothing needs parking: the answer or
/// netstackd's death ends the wait.
pub(crate) fn rpc_nonce(
    net: &KernelClient,
    req: &[u8],
    expect_rsp_op: u8,
    nonce: u64,
    reply_recv_slot: u32,
    reply_send_slot: u32,
) -> core::result::Result<[u8; 512], ()> {
    let reply = SlotPair::new(reply_send_slot, reply_recv_slot);
    let mut buf = [0u8; 512];
    nexus_ipc::exchange::call_matching(net.slots().0, reply, req, &mut buf, |rsp| {
        response_matches(rsp, expect_rsp_op, nonce).then_some(())
    })
    .map_err(|_| ())?;
    Ok(buf)
}
