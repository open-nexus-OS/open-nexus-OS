// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Shared reply pump for the `updated` submodule — RFC-0019
//!   nonce-correlated shared-inbox reply consumer:
//!     * `updated_send_with_reply` -- nonblocking, deadline-bounded request.
//!     * `updated_expect_status`   -- response framing + status validation.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU marker ladder (just test-os) — ota phase.
//!
//! ADR: docs/adr/0027-selftest-client-two-axis-architecture.md

extern crate alloc;

use alloc::collections::VecDeque;
use alloc::vec::Vec;

use nexus_abi::{yield_, MsgHeader};
use nexus_ipc::KernelClient;

use crate::markers::{emit_byte, emit_bytes, emit_hex_u64, emit_line};

pub(crate) fn updated_expect_status<'a>(
    rsp: &'a [u8],
    op: u8,
) -> core::result::Result<&'a [u8], ()> {
    if rsp.len() < 7 {
        emit_line(crate::markers::M_SELFTEST_UPDATED_RSP_SHORT);
        return Err(());
    }
    if rsp[0] != nexus_abi::updated::MAGIC0
        || rsp[1] != nexus_abi::updated::MAGIC1
        || rsp[2] != nexus_abi::updated::VERSION
    {
        emit_bytes(crate::markers::M_SELFTEST_UPDATED_RSP_MAGIC.as_bytes());
        emit_hex_u64(rsp[0] as u64);
        emit_byte(b' ');
        emit_hex_u64(rsp[1] as u64);
        emit_byte(b' ');
        emit_hex_u64(rsp[2] as u64);
        emit_byte(b'\n');
        return Err(());
    }
    if rsp[3] != (op | 0x80) || rsp[4] != nexus_abi::updated::STATUS_OK {
        emit_bytes(crate::markers::M_SELFTEST_UPDATED_RSP_STATUS.as_bytes());
        emit_hex_u64(rsp[3] as u64);
        emit_byte(b' ');
        emit_hex_u64(rsp[4] as u64);
        emit_byte(b'\n');
        return Err(());
    }
    let len = u16::from_le_bytes([rsp[5], rsp[6]]) as usize;
    if rsp.len() != 7 + len {
        emit_line(crate::markers::M_SELFTEST_UPDATED_RSP_LEN_MISMATCH);
        return Err(());
    }
    Ok(&rsp[7..])
}

pub(crate) fn updated_send_with_reply(
    client: &KernelClient,
    reply_send_slot: u32,
    reply_recv_slot: u32,
    op: u8,
    frame: &[u8],
    pending: &mut VecDeque<Vec<u8>>,
) -> core::result::Result<alloc::vec::Vec<u8>, ()> {
    if reply_send_slot == 0 || reply_recv_slot == 0 {
        return Err(());
    }

    // Drain any stale messages on the shared reply inbox before starting a new exchange.
    // IMPORTANT: do NOT discard them; buffer them so late/out-of-order replies remain consumable.
    for _ in 0..256 {
        let mut hdr = MsgHeader::new(0, 0, 0, 0, 0);
        let mut buf = [0u8; 512];
        match nexus_abi::ipc_recv_v1(
            reply_recv_slot,
            &mut hdr,
            &mut buf,
            nexus_abi::IPC_SYS_NONBLOCK | nexus_abi::IPC_SYS_TRUNCATE,
            0,
        ) {
            Ok(n) => {
                let n = n as usize;
                // Only buffer frames that look like an `updated` reply; other noise is ignored.
                if n >= 4
                    && buf[0] == nexus_abi::updated::MAGIC0
                    && buf[1] == nexus_abi::updated::MAGIC1
                    && buf[2] == nexus_abi::updated::VERSION
                    && (buf[3] & 0x80) != 0
                {
                    if pending.len() >= 16 {
                        let _ = pending.pop_front();
                    }
                    pending.push_back(buf[..n].to_vec());
                }
                continue;
            }
            Err(nexus_abi::IpcError::QueueEmpty) => break,
            Err(_) => break,
        }
    }

    // Also drain the normal updated reply channel (client recv slot). This is a compatibility
    // fallback for bring-up where CAP_MOVE/@reply delivery can be flaky or unavailable.
    let (_updated_send_slot, updated_recv_slot) = client.slots();
    for _ in 0..256 {
        let mut hdr = MsgHeader::new(0, 0, 0, 0, 0);
        let mut buf = [0u8; 512];
        match nexus_abi::ipc_recv_v1(
            updated_recv_slot,
            &mut hdr,
            &mut buf,
            nexus_abi::IPC_SYS_NONBLOCK | nexus_abi::IPC_SYS_TRUNCATE,
            0,
        ) {
            Ok(n) => {
                let n = n as usize;
                if n >= 4
                    && buf[0] == nexus_abi::updated::MAGIC0
                    && buf[1] == nexus_abi::updated::MAGIC1
                    && buf[2] == nexus_abi::updated::VERSION
                    && (buf[3] & 0x80) != 0
                {
                    if pending.len() >= 16 {
                        let _ = pending.pop_front();
                    }
                    pending.push_back(buf[..n].to_vec());
                }
                continue;
            }
            Err(nexus_abi::IpcError::QueueEmpty) => break,
            Err(_) => break,
        }
    }

    // Shared reply inbox: replies can arrive out-of-order across ops.
    if let Some(pos) = pending.iter().position(|rsp| {
        rsp.len() >= 4
            && rsp[0] == nexus_abi::updated::MAGIC0
            && rsp[1] == nexus_abi::updated::MAGIC1
            && rsp[2] == nexus_abi::updated::VERSION
            && rsp[3] == (op | 0x80)
    }) {
        if let Some(rsp) = pending.remove(pos) {
            return Ok(rsp);
        }
    }

    // Prefer plain request/response for bring-up stability; CAP_MOVE remains available but is
    // not required to validate the OTA stage/switch/health markers.
    //
    // IMPORTANT: Avoid kernel deadline-based blocking IPC in bring-up; we've observed
    // deadline semantics that can stall indefinitely. Use NONBLOCK + bounded retry.
    let (updated_send_slot, _updated_recv_slot2) = client.slots();
    {
        let hdr = MsgHeader::new(0, 0, 0, 0, frame.len() as u32);
        let start_ns = nexus_abi::nsec().map_err(|_| ())?;
        let budget_ns: u64 = if op == nexus_abi::updated::OP_STAGE {
            2_000_000_000 // 2s to enqueue a stage request under QEMU
        } else {
            500_000_000 // 0.5s for small ops
        };
        let deadline_ns = start_ns.saturating_add(budget_ns);
        let mut i: usize = 0;
        loop {
            match nexus_abi::ipc_send_v1(
                updated_send_slot,
                &hdr,
                frame,
                nexus_abi::IPC_SYS_NONBLOCK,
                0,
            ) {
                Ok(_) => break,
                Err(nexus_abi::IpcError::QueueFull) => {
                    if (i & 0x7f) == 0 {
                        let now = nexus_abi::nsec().map_err(|_| ())?;
                        if now >= deadline_ns {
                            emit_line(crate::markers::M_SELFTEST_UPDATED_SEND_TIMEOUT);
                            return Err(());
                        }
                    }
                    let _ = yield_();
                }
                Err(_) => {
                    emit_line(crate::markers::M_SELFTEST_UPDATED_SEND_FAIL);
                    return Err(());
                }
            }
            i = i.wrapping_add(1);
        }
    }
    // Give the receiver a chance to run immediately after enqueueing (cooperative scheduler).
    let _ = yield_();
    let mut hdr = MsgHeader::new(0, 0, 0, 0, 0);
    let mut buf = [0u8; 512];
    let mut logged_noise = false;
    // Time-bounded nonblocking receive loop (explicitly yields).
    //
    // NOTE: Kernel deadline semantics for ipc_recv_v1 have been flaky in bring-up; using an
    // explicit nsec()-bounded loop keeps the QEMU smoke run deterministic and bounded (RFC-0013).
    let start_ns = nexus_abi::nsec().map_err(|_| ())?;
    let budget_ns: u64 = if op == nexus_abi::updated::OP_STAGE {
        30_000_000_000 // 30s (stage includes digest + signature verify; allow for QEMU jitter)
    } else {
        5_000_000_000 // 5s (switch/health can involve cross-service publication)
    };
    let deadline_ns = start_ns.saturating_add(budget_ns);
    let mut i: usize = 0;
    loop {
        if (i & 0x7f) == 0 {
            let now = nexus_abi::nsec().map_err(|_| ())?;
            if now >= deadline_ns {
                break;
            }
        }
        match nexus_abi::ipc_recv_v1(
            updated_recv_slot,
            &mut hdr,
            &mut buf,
            nexus_abi::IPC_SYS_NONBLOCK | nexus_abi::IPC_SYS_TRUNCATE,
            0,
        ) {
            Ok(n) => {
                let n = n as usize;
                if n >= 4
                    && buf[0] == nexus_abi::updated::MAGIC0
                    && buf[1] == nexus_abi::updated::MAGIC1
                    && buf[2] == nexus_abi::updated::VERSION
                    && (buf[3] & 0x80) != 0
                {
                    if buf[3] == (op | 0x80) {
                        return Ok(buf[..n].to_vec());
                    }
                    if !logged_noise {
                        logged_noise = true;
                        emit_bytes(crate::markers::M_SELFTEST_UPDATED_RSP_OTHER_OP_0X.as_bytes());
                        emit_hex_u64(buf[3] as u64);
                        if n >= 5 {
                            emit_bytes(b" st=0x");
                            emit_hex_u64(buf[4] as u64);
                        }
                        emit_byte(b'\n');
                    }
                    if pending.len() >= 16 {
                        let _ = pending.pop_front();
                    }
                    pending.push_back(buf[..n].to_vec());
                }
            }
            Err(nexus_abi::IpcError::QueueEmpty) => {
                let _ = yield_();
            }
            Err(_) => return Err(()),
        }
        i = i.wrapping_add(1);
    }
    emit_line(crate::markers::M_SELFTEST_UPDATED_RECV_TIMEOUT);
    Err(())
}
