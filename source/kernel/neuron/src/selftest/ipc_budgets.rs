// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the router's byte budgets on an isolated local router — the global queued-bytes cap
//! and the per-owner cap (RFC-0096): the send past the cap is `NoSpace`, a drain frees it. Pure
//! (no task context); split out of `selftest/mod.rs` (module-size ratchet).
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `KSELFTEST: ipc global bytes budget ok` / `ipc owner bytes budget ok` on every
//!   QEMU lane

use crate::ipc::header::MessageHeader;
use alloc::vec;

pub(super) fn run_ipc_global_bytes_budget_selftest() {
    // Validate global router queued-bytes budget using an isolated local router instance.
    // Global budget=512, but endpoint budget (depth=2) would allow 1024 bytes. The second send
    // must fail with NoSpace due to the global cap.
    let mut local = crate::ipc::Router::new_with_global_bytes_budget(0, 512);
    let ep = local.create_endpoint(2, None).unwrap();

    let hdr = MessageHeader::new(0, ep, 0, 0, 512);
    let msg = crate::ipc::Message::new(hdr, crate::ipc::Payload::from_vec(vec![0u8; 512]), None);
    let _ = local.send(ep, msg);

    let hdr2 = MessageHeader::new(0, ep, 0, 0, 1);
    let msg2 = crate::ipc::Message::new(hdr2, crate::ipc::Payload::from_slice(&[0u8; 1]), None);
    match local.send(ep, msg2) {
        Err(crate::ipc::IpcError::NoSpace) => {
            // Drain frees bytes; subsequent send should work.
            let _ = local.recv(ep);
            let hdr3 = MessageHeader::new(0, ep, 0, 0, 1);
            let msg3 = crate::ipc::Message::new(hdr3, crate::ipc::Payload::empty(), None);
            match local.send(ep, msg3) {
                Ok(()) => log_info!(target: "selftest", "KSELFTEST: ipc global bytes budget ok"),
                other => {
                    log_error!(target: "selftest", "KSELFTEST: ipc global bytes budget FAIL: {:?}", other)
                }
            }
        }
        other => {
            log_error!(target: "selftest", "KSELFTEST: ipc global bytes budget FAIL: {:?}", other)
        }
    }
}

pub(super) fn run_ipc_owner_bytes_budget_selftest() {
    // Owner budget=512 bytes; global budget is large enough not to interfere.
    let mut local = crate::ipc::Router::new_with_bytes_budgets(0, 4096, 512);
    let owner: u32 = 7;
    let ep1 = local.create_endpoint(2, Some(owner)).unwrap();
    let ep2 = local.create_endpoint(2, Some(owner)).unwrap();

    // Fill owner budget with one 512-byte message on ep1.
    let hdr = MessageHeader::new(0, ep1, 0, 0, 512);
    let msg = crate::ipc::Message::new(hdr, crate::ipc::Payload::from_vec(vec![0u8; 512]), None);
    let _ = local.send(ep1, msg);

    // Next send to a different endpoint owned by same PID must fail due to owner cap.
    let hdr2 = MessageHeader::new(0, ep2, 0, 0, 1);
    let msg2 = crate::ipc::Message::new(hdr2, crate::ipc::Payload::from_slice(&[0u8; 1]), None);
    match local.send(ep2, msg2) {
        Err(crate::ipc::IpcError::NoSpace) => {
            // Drain frees bytes; subsequent send should work.
            let _ = local.recv(ep1);
            let hdr3 = MessageHeader::new(0, ep2, 0, 0, 1);
            let msg3 = crate::ipc::Message::new(hdr3, crate::ipc::Payload::empty(), None);
            match local.send(ep2, msg3) {
                Ok(()) => log_info!(target: "selftest", "KSELFTEST: ipc owner bytes budget ok"),
                other => {
                    log_error!(target: "selftest", "KSELFTEST: ipc owner bytes budget FAIL: {:?}", other)
                }
            }
        }
        other => {
            log_error!(target: "selftest", "KSELFTEST: ipc owner bytes budget FAIL: {:?}", other)
        }
    }
}
