// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Helper to fetch the local IPv4 address from `netstackd` for use by
//! DSoftBus / discovery probes. Extracted verbatim from the previous
//! monolithic `os_lite` block in `main.rs` (TASK-0023B / RFC-0038 phase 1,
//! cut 2). No behavior, marker, or reject-path change.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal (binary crate)
//! TEST_COVERAGE: Indirect via QEMU `just test-os` (DSoftBus discovery path).
//! ADR: docs/adr/0027-selftest-client-two-axis-architecture.md, docs/rfcs/RFC-0038-*.md

use nexus_service_topology::slots::selftest_client::REPLY;

use super::super::ipc::clients::cached_netstackd_client;

pub(crate) fn netstackd_local_addr() -> Option<[u8; 4]> {
    const MAGIC0: u8 = b'N';
    const MAGIC1: u8 = b'S';
    const VERSION: u8 = 1;
    const OP_LOCAL_ADDR: u8 = 10;
    const STATUS_OK: u8 = 0;

    let net = cached_netstackd_client().ok()?;
    let req = [MAGIC0, MAGIC1, VERSION, OP_LOCAL_ADDR];
    // This used to take the FIRST frame off the harness' shared inbox and read bytes 5..9 out
    // of it, whatever protocol it belonged to; the answer is the frame that carries OUR op
    // (TASK-0054C P2-c), and the 5 000-spin budget is gone with the poll.
    let mut buf = [0u8; 512];
    nexus_ipc::exchange::call_matching(net.slots().0, REPLY, &req, &mut buf, |rsp| {
        (rsp.len() >= 9
            && rsp[0] == MAGIC0
            && rsp[1] == MAGIC1
            && rsp[2] == VERSION
            && rsp[3] == (OP_LOCAL_ADDR | 0x80)
            && rsp[4] == STATUS_OK)
            .then(|| [rsp[5], rsp[6], rsp[7], rsp[8]])
    })
    .ok()
}
