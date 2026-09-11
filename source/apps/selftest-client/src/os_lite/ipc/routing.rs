// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Service-routing helpers (`routing_v1_get`, `route_with_retry`)
//! used by selftest probes to obtain `KernelClient` instances for core OS
//! services. Extracted verbatim from the previous monolithic `os_lite` block
//! in `main.rs` (TASK-0023B / RFC-0038 phase 1, cut 3). No behavior, marker,
//! or reject-path change.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal (binary crate)
//! TEST_COVERAGE: QEMU marker ladder via `just test-os` (full ladder).
//! ADR: docs/adr/0027-selftest-client-two-axis-architecture.md, docs/rfcs/RFC-0038-*.md

use nexus_abi::{yield_, MsgHeader};
use nexus_ipc::KernelClient;
use nexus_service_topology::{route_matches, route_slots, ServiceId, CTRL_SLOTS};

use crate::markers::{emit_byte, emit_bytes};

pub(crate) fn routing_v1_get(target: &str) -> core::result::Result<(u8, u32, u32), ()> {
    // Routing v1 (init-lite responder) over the fleet control channel:
    // GET: [R, T, ver, OP_ROUTE_GET, name_len:u8, name...]
    // RSP: [R, T, ver, OP_ROUTE_RSP, status, send_slot:u32le, recv_slot:u32le]
    let name = target.as_bytes();
    static ROUTE_NONCE: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(1);
    let nonce = ROUTE_NONCE.fetch_add(1, core::sync::atomic::Ordering::Relaxed);

    // Routing v1+nonce extension:
    // GET: [R,T,1,OP_ROUTE_GET, name_len, name..., nonce:u32le]
    // RSP: [R,T,1,OP_ROUTE_RSP, status, send_slot:u32le, recv_slot:u32le, nonce:u32le]
    let mut req = [0u8; 5 + nexus_abi::routing::MAX_SERVICE_NAME_LEN + 4];
    let base_len =
        nexus_abi::routing::encode_route_get(name, &mut req[..5 + name.len()]).ok_or(())?;
    req[base_len..base_len + 4].copy_from_slice(&nonce.to_le_bytes());
    let req_len = base_len + 4;
    let hdr = MsgHeader::new(0, 0, 0, 0, req_len as u32);
    // Deterministic: send once, then wait for the nonce-correlated RSP.
    // Avoid flooding the control channel with duplicate ROUTE_GET requests.
    let start = nexus_abi::nsec().map_err(|_| ())?;
    let deadline = start.saturating_add(500_000_000); // 500ms
    let mut i: usize = 0;
    loop {
        match nexus_abi::ipc_send_v1(
            CTRL_SLOTS.send,
            &hdr,
            &req[..req_len],
            nexus_abi::IPC_SYS_NONBLOCK,
            0,
        ) {
            Ok(_) => break,
            Err(nexus_abi::IpcError::QueueFull) => {
                if (i & 0x7f) == 0 {
                    let now = nexus_abi::nsec().map_err(|_| ())?;
                    if now >= deadline {
                        return Err(());
                    }
                }
                let _ = yield_();
            }
            Err(_) => return Err(()),
        }
        i = i.wrapping_add(1);
    }

    let mut rh = MsgHeader::new(0, 0, 0, 0, 0);
    let mut buf = [0u8; 32];
    let mut j: usize = 0;
    loop {
        if (j & 0x7f) == 0 {
            let now = nexus_abi::nsec().map_err(|_| ())?;
            if now >= deadline {
                return Err(());
            }
        }
        match nexus_abi::ipc_recv_v1(
            CTRL_SLOTS.recv,
            &mut rh,
            &mut buf,
            nexus_abi::IPC_SYS_NONBLOCK | nexus_abi::IPC_SYS_TRUNCATE,
            0,
        ) {
            Ok(n) => {
                let n = n as usize;
                if n != 17 {
                    // Ignore legacy/non-correlated control frames.
                    let _ = yield_();
                    continue;
                }
                let got_nonce = u32::from_le_bytes([buf[13], buf[14], buf[15], buf[16]]);
                if got_nonce != nonce {
                    continue;
                }
                if let Some((status, send, recv)) = nexus_abi::routing::decode_route_rsp(&buf[..13])
                {
                    return Ok((status, send, recv));
                }
            }
            Err(nexus_abi::IpcError::QueueEmpty) => {
                let _ = yield_();
            }
            Err(_) => return Err(()),
        }
        j = j.wrapping_add(1);
    }
}

/// A client for a route the topology declares for this harness, obtained from init's route
/// responder and CHECKED against the declaration (TASK-0324 P4f-5). The
/// `SELFTEST: ipc routing <svc> ok` markers that follow a successful call therefore prove that the
/// responder serves exactly the declared slots; they used to follow a hardcoded slot table and
/// proved only that a client object could be built. An undeclared name is refused without asking.
/// The ask stays bounded: the responder answers once wiring is done (the stage fence, P5, makes
/// that a single ask).
pub(crate) fn route_with_retry(name: &str) -> core::result::Result<KernelClient, ()> {
    let target = ServiceId::from_name(name.as_bytes()).ok_or(())?;
    route_slots(ServiceId::SelftestClient, target).ok_or(())?;
    for _ in 0..64 {
        if let Ok((status, send, recv)) = routing_v1_get(name) {
            if status == nexus_abi::routing::STATUS_OK {
                if !route_matches(ServiceId::SelftestClient, target, send, recv) {
                    emit_bytes(
                        crate::markers::M_SELFTEST_ROUTE_DIVERGES_FROM_DECLARATION_FAIL_SVC
                            .as_bytes(),
                    );
                    emit_bytes(name.as_bytes());
                    emit_byte(b'\n');
                    return Err(());
                }
                return KernelClient::new_with_slots(send, recv).map_err(|_| ());
            }
        }
        let _ = yield_();
    }
    Err(())
}
