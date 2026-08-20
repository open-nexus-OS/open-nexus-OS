// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: logd server-endpoint routing (split out of `os_lite.rs` under
//! the structure ratchet, TASK-0049C). Resolves logd's own server slots via
//! the init responder with bounded nonce-correlated retries; the serve loop
//! falls back to the deterministic slots 3/4 the declarative arm provisions.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: QEMU ladder (`logd: ready`).
//! ADR: docs/adr/0017-service-architecture.md

use nexus_abi::yield_;
use nexus_ipc::KernelServer;

pub(crate) fn route_logd_blocking() -> Option<KernelServer> {
    const CTRL_SEND_SLOT: u32 = 1;
    const CTRL_RECV_SLOT: u32 = 2;
    let name = b"logd";
    static ROUTE_NONCE: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(1);
    let nonce = ROUTE_NONCE.fetch_add(1, core::sync::atomic::Ordering::Relaxed);

    // Routing v1+nonce extension:
    // GET: [R,T,1,OP_ROUTE_GET, name_len, name..., nonce:u32le]
    // RSP: [R,T,1,OP_ROUTE_RSP, status, send_slot:u32le, recv_slot:u32le, nonce:u32le]
    let mut req = [0u8; 5 + nexus_abi::routing::MAX_SERVICE_NAME_LEN + 4];
    let base_len = nexus_abi::routing::encode_route_get(name, &mut req[..5 + name.len()])?;
    req[base_len..base_len + 4].copy_from_slice(&nonce.to_le_bytes());
    let req_len = base_len + 4;
    let hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, req_len as u32);
    // Bounded probing: if routing isn't available yet, fall back quickly to deterministic slots.
    for _ in 0..64 {
        // Avoid blocking IPC on the routing control plane (can deadlock under cooperative scheduling).
        if nexus_abi::ipc_send_v1(
            CTRL_SEND_SLOT,
            &hdr,
            &req[..req_len],
            nexus_abi::IPC_SYS_NONBLOCK,
            0,
        )
        .is_err()
        {
            let _ = yield_();
            continue;
        }
        let mut rh = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
        let mut buf = [0u8; 32];
        match nexus_abi::ipc_recv_v1(
            CTRL_RECV_SLOT,
            &mut rh,
            &mut buf,
            nexus_abi::IPC_SYS_NONBLOCK | nexus_abi::IPC_SYS_TRUNCATE,
            0,
        ) {
            Ok(n) => {
                let n = n as usize;
                if n == 17 {
                    let got_nonce = u32::from_le_bytes([buf[13], buf[14], buf[15], buf[16]]);
                    if got_nonce != nonce {
                        let _ = yield_();
                        continue;
                    }
                }
                if n != 17 {
                    let _ = yield_();
                    continue;
                }
                let (status, send_slot, recv_slot) =
                    nexus_abi::routing::decode_route_rsp(&buf[..13])?;
                if status != nexus_abi::routing::STATUS_OK {
                    let _ = yield_();
                    continue;
                }
                return KernelServer::new_with_slots(recv_slot, send_slot).ok();
            }
            Err(nexus_abi::IpcError::QueueEmpty) => {
                let _ = yield_();
            }
            Err(_) => {}
        }
    }
    None
}
