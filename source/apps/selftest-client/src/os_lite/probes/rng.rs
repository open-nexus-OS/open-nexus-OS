// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Kernel RNG entropy probe (TASK-0006). Exercises the rngd entropy
//!   request path with bounded payloads and an oversized-request reject.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU marker ladder (just test-os) — bringup phase consumes
//!   `rng_entropy_selftest`.
//!
//! ADR: docs/adr/0027-selftest-client-two-axis-architecture.md

extern crate alloc;

use alloc::vec::Vec;

use nexus_abi::yield_;
use nexus_ipc::{Client, KernelClient, Wait as IpcWait};

use crate::markers::{emit_bytes, emit_hex_u64, emit_line};

pub(crate) fn rng_entropy_selftest() {
    // Build rngd GET_ENTROPY request for 32 bytes
    // Request: [R, G, 1, OP_GET_ENTROPY=1, nonce:u32le, n:u16le]
    let nonce = (nexus_abi::nsec().unwrap_or(0) as u32) ^ 0xA5A5_5A5A;
    let mut req = Vec::with_capacity(10);
    req.push(b'R'); // MAGIC0
    req.push(b'G'); // MAGIC1
    req.push(1); // VERSION
    req.push(1); // OP_GET_ENTROPY
    req.extend_from_slice(&nonce.to_le_bytes());
    req.extend_from_slice(&32u16.to_le_bytes()); // Request 32 bytes

    // Connect to rngd using the deterministic slots distributed by init-lite.
    const RNGD_SEND_SLOT: u32 = 0x1d;
    const RNGD_RECV_SLOT: u32 = 0x1e;
    let client = match KernelClient::new_with_slots(RNGD_SEND_SLOT, RNGD_RECV_SLOT) {
        Ok(c) => c,
        Err(_) => {
            emit_line("SELFTEST: rng entropy FAIL (no slots)");
            return;
        }
    };

    let wait = IpcWait::Timeout(core::time::Duration::from_millis(500));
    emit_line("SELFTEST: rng entropy send");
    if client.send(&req, wait).is_err() {
        emit_line("SELFTEST: rng entropy FAIL (send)");
        return;
    }

    // Receive response on the dedicated rngd reply inbox
    let start = nexus_abi::nsec().unwrap_or(0);
    let deadline = start.saturating_add(500_000_000);
    let mut spins: u32 = 0;
    const MAX_SPINS: u32 = 200_000;
    loop {
        let now = nexus_abi::nsec().unwrap_or(0);
        if now >= deadline || spins >= MAX_SPINS {
            emit_line("SELFTEST: rng entropy FAIL (recv)");
            return;
        }
        match client.recv(IpcWait::NonBlocking) {
            Ok(rsp) => {
                // Response: [R, G, 1, OP|0x80, STATUS, nonce:u32le, entropy...]
                if rsp.len() < 9 || rsp[0] != b'R' || rsp[1] != b'G' || rsp[2] != 1 {
                    // Ignore unrelated frames.
                    continue;
                }
                if rsp[3] != (1 | 0x80) {
                    emit_line("SELFTEST: rng entropy FAIL (wrong op)");
                    return;
                }
                if rsp[4] != 0 {
                    emit_bytes(b"SELFTEST: rng entropy FAIL (status=");
                    emit_hex_u64(rsp[4] as u64);
                    emit_line(")");
                    return;
                }
                let got_nonce = u32::from_le_bytes([rsp[5], rsp[6], rsp[7], rsp[8]]);
                if got_nonce != nonce {
                    continue; // unrelated reply
                }
                let entropy_len = rsp.len() - 9;
                if entropy_len != 32 {
                    emit_bytes(b"SELFTEST: rng entropy FAIL (len=");
                    emit_hex_u64(entropy_len as u64);
                    emit_line(")");
                    return;
                }
                // SECURITY: Do NOT log entropy bytes!
                emit_line("SELFTEST: rng entropy ok");
                return;
            }
            Err(_) => {
                let _ = yield_();
            }
        }
        spins = spins.wrapping_add(1);
    }
}

/// Test rngd rejects oversized entropy requests.
/// Proves: bounds enforcement on entropy length.
pub(crate) fn rng_entropy_oversized_selftest() {
    let nonce = (nexus_abi::nsec().unwrap_or(0) as u32) ^ 0x5A5A_A5A5;
    let mut req = Vec::with_capacity(10);
    req.push(b'R');
    req.push(b'G');
    req.push(1);
    req.push(1);
    req.extend_from_slice(&nonce.to_le_bytes());
    req.extend_from_slice(&257u16.to_le_bytes());

    const RNGD_SEND_SLOT: u32 = 0x1d;
    const RNGD_RECV_SLOT: u32 = 0x1e;
    let client = match KernelClient::new_with_slots(RNGD_SEND_SLOT, RNGD_RECV_SLOT) {
        Ok(c) => c,
        Err(_) => {
            emit_line("SELFTEST: rng entropy oversized FAIL (no slots)");
            return;
        }
    };

    let wait = IpcWait::Timeout(core::time::Duration::from_millis(500));
    if client.send(&req, wait).is_err() {
        emit_line("SELFTEST: rng entropy oversized FAIL (send)");
        return;
    }

    let start = nexus_abi::nsec().unwrap_or(0);
    let deadline = start.saturating_add(500_000_000);
    let mut spins: u32 = 0;
    const MAX_SPINS: u32 = 200_000;
    loop {
        let now = nexus_abi::nsec().unwrap_or(0);
        if now >= deadline || spins >= MAX_SPINS {
            emit_line("SELFTEST: rng entropy oversized FAIL (recv)");
            return;
        }
        match client.recv(IpcWait::NonBlocking) {
            Ok(rsp) => {
                if rsp.len() < 9 || rsp[0] != b'R' || rsp[1] != b'G' || rsp[2] != 1 {
                    continue;
                }
                if rsp[3] != (1 | 0x80) {
                    emit_line("SELFTEST: rng entropy oversized FAIL (wrong op)");
                    return;
                }
                let got_nonce = u32::from_le_bytes([rsp[5], rsp[6], rsp[7], rsp[8]]);
                if got_nonce != nonce {
                    continue;
                }
                if rsp[4] != 1 {
                    emit_bytes(b"SELFTEST: rng entropy oversized FAIL (status=");
                    emit_hex_u64(rsp[4] as u64);
                    emit_line(")");
                    return;
                }
                emit_line("SELFTEST: rng entropy oversized ok");
                return;
            }
            Err(_) => {
                let _ = yield_();
            }
        }
        spins = spins.wrapping_add(1);
    }
}
