// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The interaction-net job (Phase E) — `JOB_INET_TREE_SUM` served
//! on the nexus-inet backend: round-based parallel reduction on the shared
//! workpool (each round = one deterministic-chunk run over the current redex
//! list; locked next-round deque per TASK-0277; confluence keeps the result
//! independent of merge order). Split from os_lite.rs (no god files).
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: nexus-inet host tests (rules, equality matrix); QEMU
//!   markers `SELFTEST: inet determinism/bounded/parallel exec ok`.
//! ADR: docs/adr/0045-pinched-compute-broker-and-backends.md

use alloc::sync::Arc;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use nexus_sync::SpinLock;

use crate::os_lite::{emit_line, finish, vmo_capacity, POOL_READY};
use crate::protocol::*;
use crate::{INET_ARENA_NODES, MAX_INET_DEPTH, PINCHED_WORKERS};

/// The in-flight interaction-net round. Same publish discipline as SVG_JOB:
/// the server stores the Arc before signalling the job fence; workers clone
/// it and reduce their deterministic chunk of the round's redex list.
static INET_ROUND: SpinLock<Option<Arc<InetRound>>> = SpinLock::new(None);

struct InetRound {
    net: Arc<nexus_inet::Arena>,
    redexes: alloc::vec::Vec<nexus_inet::Redex>,
    /// Next-round redexes, merged under a short lock (TASK-0277: locked
    /// deques, no lock-free experiments). Confluence keeps the RESULT
    /// independent of merge order.
    next: SpinLock<alloc::vec::Vec<nexus_inet::Redex>>,
    /// Per-worker interaction counters (parallel-dispatch proof) and a
    /// sticky error flag (any rule failure fails the whole job, loudly).
    reductions: [AtomicU32; nexus_workpool::MAX_WORKERS],
    failed: AtomicBool,
}

/// The `JOB_INET_TREE_SUM` round job: reduce our chunk of this round's
/// redexes; freshly created redexes go into the shared next-round list.
extern "C" fn job_inet_round(start: usize, end: usize, _ctx: *mut u8) {
    let round = INET_ROUND.lock().clone();
    let Some(round) = round else {
        return;
    };
    let mut out = nexus_inet::RoundOut::default();
    if nexus_inet::reduce_chunk(&round.net, &round.redexes, start, end, &mut out).is_err() {
        round.failed.store(true, Ordering::Release);
        return;
    }
    // Worker identity for the counter: derive from the chunk start (chunking
    // is deterministic, chunks are disjoint — start maps to at most one idx).
    let idx = if start == 0 { 0 } else { 1 };
    round.reductions[idx].fetch_add(out.reductions, Ordering::AcqRel);
    round.next.lock().extend(out.new_redexes);
}

enum RoundErr {
    /// The pool refused before any work started — inline is safe.
    PoolIdle,
    /// The round started but timed out — worker state unknown, abort.
    PoolTimeout,
    /// An interaction hit a pair with no rule.
    Stuck,
}

/// One parallel reduction round: publish the round context, run the workers
/// over the redex list (deterministic chunks), collect the next round.
fn run_inet_round(
    net: &Arc<nexus_inet::Arena>,
    redexes: &[nexus_inet::Redex],
    counts: &mut [u32; 2],
) -> Result<(u32, alloc::vec::Vec<nexus_inet::Redex>), RoundErr> {
    let round = Arc::new(InetRound {
        net: net.clone(),
        redexes: redexes.to_vec(),
        next: SpinLock::new(alloc::vec::Vec::new()),
        reductions: [const { AtomicU32::new(0) }; nexus_workpool::MAX_WORKERS],
        failed: AtomicBool::new(false),
    });
    *INET_ROUND.lock() = Some(round.clone());
    let run = nexus_workpool::run(
        round.redexes.len(),
        job_inet_round,
        core::ptr::null_mut(),
        6_000_000_000,
    );
    *INET_ROUND.lock() = None;
    match run {
        Ok(()) => {}
        Err(nexus_workpool::PoolError::Timeout) => return Err(RoundErr::PoolTimeout),
        Err(_) => return Err(RoundErr::PoolIdle),
    }
    if round.failed.load(Ordering::Acquire) {
        return Err(RoundErr::Stuck);
    }
    let mut reds: u32 = 0;
    for (idx, c) in round.reductions.iter().enumerate() {
        let v = c.load(Ordering::Acquire);
        reds = reds.saturating_add(v);
        if idx < 2 {
            counts[idx] = counts[idx].saturating_add(v);
        }
    }
    let next = core::mem::take(&mut *round.next.lock());
    Ok((reds, next))
}

/// Serves one `JOB_INET_TREE_SUM` (Phase E): build the add-tree net, reduce
/// it round by round on the workpool (each round = one deterministic-chunk
/// run over the current redex list), read the folded number off the root.
/// Bounded everywhere: depth-capped, arena-capped, round-capped; the inline
/// fallback reports `workers = 0`.
pub(crate) fn handle_inet_tree_sum(vmo: u32, depth: usize) {
    const OUT_LEN: usize = 16; // result:i64le + red_w0:u32le + red_w1:u32le
    if depth == 0 || depth > MAX_INET_DEPTH {
        emit_line("pinched: inet reject (depth)");
        return finish(vmo, STATUS_OVERSIZED, 0, 0);
    }
    if vmo_capacity(vmo).map_or(true, |len| len < DATA_OFFSET + OUT_LEN) {
        emit_line("pinched: inet reject (vmo capacity)");
        return finish(vmo, STATUS_OVERSIZED, 0, 0);
    }
    // Per-job allocations (net arena + redex lists) are bounded batch-path
    // allocations, same doctrine as the SVG plan.
    let net = Arc::new(nexus_inet::Arena::new(INET_ARENA_NODES));
    let (root, mut redexes) = match nexus_inet::build_tree_sum(&net, depth as u32) {
        Ok(v) => v,
        Err(_) => {
            emit_line("pinched: inet reject (arena)");
            return finish(vmo, STATUS_OVERSIZED, 0, 0);
        }
    };

    let pool = POOL_READY.load(Ordering::Acquire);
    let mut parallel = pool;
    let mut total_reds: u32 = 0;
    let mut counts = [0u32; 2];
    const MAX_ROUNDS: u32 = 10_000;
    let mut rounds = 0u32;
    while !redexes.is_empty() {
        rounds += 1;
        if rounds > MAX_ROUNDS {
            emit_line("pinched: inet FAIL (diverged)");
            return finish(vmo, STATUS_BAD_INPUT, 0, 0);
        }
        if parallel && redexes.len() >= 2 {
            match run_inet_round(&net, &redexes, &mut counts) {
                Ok((reds, next)) => {
                    total_reds = total_reds.saturating_add(reds);
                    redexes = next;
                }
                Err(RoundErr::PoolIdle) => {
                    // Nothing started (busy/poisoned/not-ready BEFORE the
                    // signal): safe to run the SAME round inline; the job
                    // then reports workers = 0 honestly.
                    parallel = false;
                }
                Err(RoundErr::PoolTimeout) => {
                    // The round DID start and did not complete — re-running
                    // it would double-reduce. Abort loudly.
                    emit_line("pinched: inet FAIL (round timeout)");
                    return finish(vmo, STATUS_IO, 0, 0);
                }
                Err(RoundErr::Stuck) => {
                    emit_line("pinched: inet FAIL (stuck)");
                    return finish(vmo, STATUS_BAD_INPUT, 0, 0);
                }
            }
        } else {
            let mut out = nexus_inet::RoundOut::default();
            if nexus_inet::reduce_chunk(&net, &redexes, 0, redexes.len(), &mut out).is_err() {
                emit_line("pinched: inet FAIL (stuck)");
                return finish(vmo, STATUS_BAD_INPUT, 0, 0);
            }
            total_reds = total_reds.saturating_add(out.reductions);
            redexes = out.new_redexes;
        }
    }

    let Some(result) = nexus_inet::root_value(&net, root) else {
        emit_line("pinched: inet FAIL (no normal form value)");
        return finish(vmo, STATUS_BAD_INPUT, 0, 0);
    };
    let workers: u32 = if parallel { PINCHED_WORKERS as u32 } else { 0 };
    let mut out = [0u8; OUT_LEN];
    out[0..8].copy_from_slice(&result.to_le_bytes());
    out[8..12].copy_from_slice(&counts[0].to_le_bytes());
    out[12..16].copy_from_slice(&counts[1].to_le_bytes());
    if nexus_abi::vmo_write(vmo, DATA_OFFSET, &out).is_err() {
        return finish(vmo, STATUS_IO, 0, 0);
    }
    finish(vmo, STATUS_OK, total_reds, workers)
}
