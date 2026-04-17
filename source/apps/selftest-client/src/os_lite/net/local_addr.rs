// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Helper to fetch the local IPv4 address from `netstackd` for use by
//! DSoftBus / discovery probes. Extracted verbatim from the previous
//! monolithic `os_lite` block in `main.rs` (TASK-0023B / RFC-0038 phase 1,
//! cut 2). No behavior, marker, or reject-path change.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal (binary crate)
//! TEST_COVERAGE: Indirect via QEMU `just test-os` (DSoftBus discovery path).
//! ADR: docs/adr/0017-service-architecture.md, docs/rfcs/RFC-0038-*.md

use nexus_abi::yield_;
use nexus_ipc::KernelClient;

use super::super::ipc::clients::{cached_netstackd_client, cached_reply_client};

pub(crate) fn netstackd_local_addr() -> Option<[u8; 4]> {
    const MAGIC0: u8 = b'N';
    const MAGIC1: u8 = b'S';
    const VERSION: u8 = 1;
    const OP_LOCAL_ADDR: u8 = 10;
    const STATUS_OK: u8 = 0;

    fn rpc(client: &KernelClient, req: &[u8]) -> core::result::Result<[u8; 512], ()> {
        let reply = cached_reply_client().map_err(|_| ())?;
        let (reply_send_slot, reply_recv_slot) = reply.slots();
        let reply_send_clone = nexus_abi::cap_clone(reply_send_slot).map_err(|_| ())?;
        client.send_with_cap_move(req, reply_send_clone).map_err(|_| ())?;
        let mut hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
        let mut buf = [0u8; 512];
        for _ in 0..5_000 {
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

    let net = cached_netstackd_client().ok()?;
    let req = [MAGIC0, MAGIC1, VERSION, OP_LOCAL_ADDR];
    let rsp = rpc(&net, &req).ok()?;
    if rsp[0] != MAGIC0
        || rsp[1] != MAGIC1
        || rsp[2] != VERSION
        || rsp[3] != (OP_LOCAL_ADDR | 0x80)
        || rsp[4] != STATUS_OK
    {
        return None;
    }
    Some([rsp[5], rsp[6], rsp[7], rsp[8]])
}
