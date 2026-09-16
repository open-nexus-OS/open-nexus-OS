// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: ICMP ping proof against `netstackd` IPC facade (TASK-0004).
//! Extracted verbatim from the previous monolithic `os_lite` block in
//! `main.rs` (TASK-0023B / RFC-0038 phase 1, cut 2). No behavior, marker, or
//! reject-path change.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal (binary crate)
//! TEST_COVERAGE: QEMU marker `SELFTEST: icmp ping ok` via `just test-os`.
//! ADR: docs/adr/0027-selftest-client-two-axis-architecture.md, docs/rfcs/RFC-0038-*.md

use nexus_service_topology::slots::selftest_client::REPLY;

use super::super::ipc::clients::cached_netstackd_client;

/// ICMP ping proof via netstackd IPC facade (TASK-0004).
pub(crate) fn icmp_ping_probe() -> core::result::Result<(), ()> {
    const MAGIC0: u8 = b'N';
    const MAGIC1: u8 = b'S';
    const VERSION: u8 = 1;
    const OP_ICMP_PING: u8 = 9;
    const STATUS_OK: u8 = 0;

    // Connect to netstackd
    let netstackd = cached_netstackd_client().map_err(|_| ())?;

    // Gateway address: 10.0.2.2 (QEMU usernet)
    let gateway_ip: [u8; 4] = [10, 0, 2, 2];
    let timeout_ms: u16 = 3000; // 3 second timeout

    // Build ICMP ping request: [magic, magic, ver, op, ip[4], timeout_ms:u16le]
    let mut req = [0u8; 10];
    req[0] = MAGIC0;
    req[1] = MAGIC1;
    req[2] = VERSION;
    req[3] = OP_ICMP_PING;
    req[4..8].copy_from_slice(&gateway_ip);
    req[8..10].copy_from_slice(&timeout_ms.to_le_bytes());

    // The answer is the frame carrying OUR op on the harness' shared inbox (TASK-0054C P2-c);
    // the wait ends on that frame or on netstackd's death, never on a clock.
    let mut buf = [0u8; 512];
    let status =
        nexus_ipc::exchange::call_matching(netstackd.slots().0, REPLY, &req, &mut buf, |rsp| {
            (rsp.len() >= 5
                && rsp[0] == MAGIC0
                && rsp[1] == MAGIC1
                && rsp[2] == VERSION
                && rsp[3] == (OP_ICMP_PING | 0x80))
                .then(|| rsp[4])
        })
        .map_err(|_| ())?;
    if status != STATUS_OK {
        return Err(());
    }

    // Ping succeeded
    Ok(())
}
