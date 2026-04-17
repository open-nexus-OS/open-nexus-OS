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

use nexus_abi::yield_;
use nexus_ipc::KernelClient;

use super::super::ipc::clients::{cached_netstackd_client, cached_reply_client};

/// ICMP ping proof via netstackd IPC facade (TASK-0004).
pub(crate) fn icmp_ping_probe() -> core::result::Result<(), ()> {
    const MAGIC0: u8 = b'N';
    const MAGIC1: u8 = b'S';
    const VERSION: u8 = 1;
    const OP_ICMP_PING: u8 = 9;
    const STATUS_OK: u8 = 0;

    fn rpc(client: &KernelClient, req: &[u8]) -> core::result::Result<[u8; 512], ()> {
        let reply = cached_reply_client().map_err(|_| ())?;
        let (reply_send_slot, reply_recv_slot) = reply.slots();
        let reply_send_clone = nexus_abi::cap_clone(reply_send_slot).map_err(|_| ())?;
        client.send_with_cap_move(req, reply_send_clone).map_err(|_| ())?;
        let mut hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
        let mut buf = [0u8; 512];
        for _ in 0..10_000 {
            match nexus_abi::ipc_recv_v1(
                reply_recv_slot,
                &mut hdr,
                &mut buf,
                nexus_abi::IPC_SYS_NONBLOCK | nexus_abi::IPC_SYS_TRUNCATE,
                0,
            ) {
                Ok(_n) => return Ok(buf),
                Err(nexus_abi::IpcError::QueueEmpty) => {
                    let _ = yield_();
                }
                Err(_) => return Err(()),
            }
        }
        Err(())
    }

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

    let rsp = rpc(&netstackd, &req)?;

    // Validate response: [magic, magic, ver, op|0x80, status, ...]
    if rsp[0] != MAGIC0 || rsp[1] != MAGIC1 || rsp[2] != VERSION {
        return Err(());
    }
    if rsp[3] != (OP_ICMP_PING | 0x80) {
        return Err(());
    }
    if rsp[4] != STATUS_OK {
        return Err(());
    }

    // Ping succeeded
    Ok(())
}
