// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Kernel-IPC request/reply bench — the first round-trip number in
//!   the repo (TASK-0054C P1, RFC-0096 Phase 1). A bounded run of the ONE
//!   exchange (`nexus_ipc::exchange::call_into`, two traps, moved reply cap,
//!   EOF-opted wait) against samgrd's `OP_PING_CAP_MOVE`: a 12-byte request
//!   and a 12-byte reply — squarely inside the ≤ 64-byte inline tier the
//!   fastpath is sized for. Prints the mean round trip; asserts nothing
//!   (the budgets are calibrated from this line before P4 asserts them).
//!   `nsec()` here MEASURES; no wait is bounded by it (RFC-0093 §7).
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU marker `SELFTEST: ipc bench (rt=<n>us n=<N>)` (ipc_kernel phase).
//!
//! Marker emission is left to the orchestrating phase (`phases::ipc_kernel`).
//!
//! ADR: docs/adr/0064-request-reply-one-trap-per-side-direct-handoff.md

use core::sync::atomic::{AtomicU64, Ordering};

use super::super::super::ipc::clients::cached_samgrd_client;

/// Exchanges per run. Small enough to stay inside samgrd's per-request
/// budget under icount, large enough that the mean is not one trap's jitter.
pub(crate) const BENCH_ROUNDS: u32 = 64;

/// One bench outcome: mean round trip in microseconds over `rounds` exchanges.
#[derive(Copy, Clone, Debug)]
pub(crate) struct BenchResult {
    pub rt_us: u64,
    pub rounds: u32,
}

/// `BENCH_ROUNDS` ping/pong exchanges with samgrd; every reply is checked for
/// the `PONG` + nonce echo so a wrong or stale frame fails the probe instead
/// of timing a lie.
pub(crate) fn ipc_bench_probe() -> core::result::Result<BenchResult, ()> {
    let reply = nexus_service_topology::slots::selftest_client::REPLY;
    let sam = cached_samgrd_client().map_err(|_| ())?;
    let (sam_send, _) = sam.slots();
    static NONCE: AtomicU64 = AtomicU64::new(0x5400);
    let mut out = [0u8; 16];
    let t0 = nexus_abi::nsec().map_err(|_| ())?;
    for _ in 0..BENCH_ROUNDS {
        let nonce = NONCE.fetch_add(1, Ordering::Relaxed);
        let mut frame = [0u8; 12];
        frame[0] = b'S';
        frame[1] = b'M';
        frame[2] = 1; // samgrd os-lite version
        frame[3] = 3; // OP_PING_CAP_MOVE
        frame[4..12].copy_from_slice(&nonce.to_le_bytes());
        // The PONG carrying OUR nonce on the harness' shared inbox (TASK-0054C P2-c).
        nexus_ipc::exchange::call_matching(sam_send, reply, &frame, &mut out, |rsp| {
            (rsp.len() == 12 && rsp[0..4] == *b"PONG" && rsp[4..12] == nonce.to_le_bytes())
                .then_some(())
        })
        .map_err(|_| ())?;
    }
    let t1 = nexus_abi::nsec().map_err(|_| ())?;
    let rt_us = t1.saturating_sub(t0) / 1_000 / u64::from(BENCH_ROUNDS);
    Ok(BenchResult { rt_us, rounds: BENCH_ROUNDS })
}
