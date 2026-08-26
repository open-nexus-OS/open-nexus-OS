// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none", feature = "os-lite"))]

//! CONTEXT: virtioblkd slot resolution over the init responder (the logd
//! `route_os` pattern): the service's OWN server slots by name, and its
//! @reply inbox — which here doubles as the driver's IRQ notify endpoint
//! (this service makes no outbound calls). Bounded nonce-correlated
//! retries; the caller falls back to the deterministic slots 3/4.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: QEMU ladder (`virtioblkd: ready`).
//! ADR: docs/adr/0044-single-blk-device-gpt-partitions-block-layer.md

use nexus_abi::yield_;
use nexus_ipc::KernelServer;

const CTRL_SEND_SLOT: u32 = 1;
const CTRL_RECV_SLOT: u32 = 2;

fn route_blocking(name: &[u8]) -> Option<(u32, u32)> {
    static ROUTE_NONCE: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(1);
    let nonce = ROUTE_NONCE.fetch_add(1, core::sync::atomic::Ordering::Relaxed);

    let mut req = [0u8; 5 + nexus_abi::routing::MAX_SERVICE_NAME_LEN + 4];
    let base_len = nexus_abi::routing::encode_route_get(name, &mut req[..5 + name.len()])?;
    req[base_len..base_len + 4].copy_from_slice(&nonce.to_le_bytes());
    let req_len = base_len + 4;
    let hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, req_len as u32);
    for _ in 0..64 {
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
                if n != 17 {
                    let _ = yield_();
                    continue;
                }
                let got_nonce = u32::from_le_bytes([buf[13], buf[14], buf[15], buf[16]]);
                if got_nonce != nonce {
                    let _ = yield_();
                    continue;
                }
                let (status, send_slot, recv_slot) =
                    nexus_abi::routing::decode_route_rsp(&buf[..13])?;
                if status != nexus_abi::routing::STATUS_OK {
                    let _ = yield_();
                    continue;
                }
                return Some((send_slot, recv_slot));
            }
            Err(nexus_abi::IpcError::QueueEmpty) => {
                let _ = yield_();
            }
            Err(_) => {}
        }
    }
    None
}

/// The service's own server endpoint (declaratively provisioned).
pub(crate) fn route_virtioblkd_blocking() -> Option<KernelServer> {
    let (send_slot, recv_slot) = route_blocking(b"virtioblkd")?;
    KernelServer::new_with_slots(recv_slot, send_slot).ok()
}
