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

/// The asserted round-trip budget for the fastpath, in microseconds
/// (TASK-0054C P6, RFC-0096 §Budgets) — judged against the FASTEST of the timed calls since
/// TASK-0333 6f-3 ([`ipc_call_budget_probe`]).
///
/// Measured 41–43 µs across every profile that runs this phase — smp1, visible,
/// reset, ota-downgrade, ota-tamper — a 5 % band. The budget is 64: enough
/// headroom that CI jitter cannot trip it, tight enough that the two-trap path
/// this replaced (192 µs) fails it three times over. A regression that puts a
/// kernel entry back into an exchange stops being a number someone has to
/// notice in a log.
///
/// It lives HERE and not in `core/trap/budgets.rs`, which RFC-0096 named,
/// because the round trip is measured by this probe and nothing in the kernel
/// can see it; a constant in the kernel would be a mirror with no way to check
/// itself. The budget the KERNEL owns — zero allocations for a short message —
/// is asserted by the counting allocator in `just test-kernel`.
pub(crate) const IPC_CALL_RT_BUDGET_US: u64 = 64;

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

/// The SAME exchange over `ipc_call` — one trap instead of two (TASK-0054C
/// P4c-1).
///
/// Same server, same 12-byte request, same 12-byte reply, same round count, so
/// the two `rt=` lines are a like-for-like comparison and not two different
/// measurements wearing the same word. samgrd answers with an ordinary send,
/// which is precisely what completes a call in the caller's registers, so this
/// times the fastpath as a service actually drives it.
pub(crate) fn ipc_call_bench_probe() -> core::result::Result<BenchResult, ()> {
    let reply = nexus_service_topology::slots::selftest_client::REPLY;
    let sam = cached_samgrd_client().map_err(|_| ())?;
    let (sam_send, _) = sam.slots();
    let mut out = [0u8; 16];
    let t0 = nexus_abi::nsec().map_err(|_| ())?;
    for _ in 0..BENCH_ROUNDS {
        ping_call(sam_send, reply.send, &mut out)?;
    }
    let t1 = nexus_abi::nsec().map_err(|_| ())?;
    let rt_us = t1.saturating_sub(t0) / 1_000 / u64::from(BENCH_ROUNDS);
    Ok(BenchResult { rt_us, rounds: BENCH_ROUNDS })
}

/// What the budget judges (TASK-0333 6f-3): every one of `BENCH_ROUNDS` calls timed alone, the
/// clock's own cost taken off. `fastest_us` is a call that nothing else interrupted — the cost
/// of the path, which is what a regression (a kernel entry back in the exchange) raises in EVERY
/// round; `mean_us` is the same rounds averaged, which the rest of the hart's work raises too
/// (a pointer burst on the one hart took the old mean-only gate from 47 to 90 µs, test-all
/// 2026-10-29) — printed, never judged.
#[derive(Copy, Clone, Debug)]
pub(crate) struct CallBudget {
    pub fastest_us: u64,
    pub mean_us: u64,
}

/// Clock reads timed back to back, to learn what a timed round pays for its own clock (each
/// read is a syscall).
const CLOCK_ROUNDS: u32 = 16;

pub(crate) fn ipc_call_budget_probe() -> core::result::Result<CallBudget, ()> {
    let reply = nexus_service_topology::slots::selftest_client::REPLY;
    let sam = cached_samgrd_client().map_err(|_| ())?;
    let (sam_send, _) = sam.slots();
    let mut clock_ns = u64::MAX;
    for _ in 0..CLOCK_ROUNDS {
        let a = nexus_abi::nsec().map_err(|_| ())?;
        let b = nexus_abi::nsec().map_err(|_| ())?;
        clock_ns = clock_ns.min(b.saturating_sub(a));
    }
    let mut out = [0u8; 16];
    let (mut fastest_ns, mut sum_ns) = (u64::MAX, 0u64);
    for _ in 0..BENCH_ROUNDS {
        let a = nexus_abi::nsec().map_err(|_| ())?;
        ping_call(sam_send, reply.send, &mut out)?;
        let b = nexus_abi::nsec().map_err(|_| ())?;
        let round_ns = b.saturating_sub(a).saturating_sub(clock_ns);
        fastest_ns = fastest_ns.min(round_ns);
        sum_ns = sum_ns.saturating_add(round_ns);
    }
    Ok(CallBudget {
        fastest_us: fastest_ns / 1_000,
        mean_us: sum_ns / 1_000 / u64::from(BENCH_ROUNDS),
    })
}

/// One `OP_PING_CAP_MOVE` over `ipc_call` — the reply cap moved, the `PONG` and OUR nonce
/// checked, so a wrong or stale frame fails the probe instead of timing a lie.
fn ping_call(sam_send: u32, reply_send: u32, out: &mut [u8; 16]) -> core::result::Result<(), ()> {
    static NONCE: AtomicU64 = AtomicU64::new(0x5C00);
    let nonce = NONCE.fetch_add(1, Ordering::Relaxed);
    let mut frame = [0u8; 12];
    frame[0] = b'S';
    frame[1] = b'M';
    frame[2] = 1; // samgrd os-lite version
    frame[3] = 3; // OP_PING_CAP_MOVE
    frame[4..12].copy_from_slice(&nonce.to_le_bytes());
    let cap = nexus_abi::cap_clone(reply_send).map_err(|_| ())?;
    let hdr = nexus_abi::MsgHeader::new(cap, 0, 0, nexus_abi::ipc_hdr::CAP_MOVE, 12);
    let n = match nexus_abi::ipc_call(sam_send, &hdr, &frame, out) {
        Ok(n) => n,
        Err(_) => {
            let _ = nexus_abi::cap_close(cap);
            return Err(());
        }
    };
    if n != 12 || out[0..4] != *b"PONG" || out[4..12] != nonce.to_le_bytes() {
        return Err(());
    }
    Ok(())
}
