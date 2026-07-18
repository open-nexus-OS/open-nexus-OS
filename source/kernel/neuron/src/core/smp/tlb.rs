// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: A5 TLB shootdown — epoch-based, allocation-free, deterministic.
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU SMP proofs (tlb shootdown ok / counterfactual / skipped)
//! PUBLIC API: shootdown_all(), poll_mailbox(), selftest evidence accessors
//! INVARIANTS:
//!   - Correctness-class IPIs: never throttled, never dropped (docs/
//!     architecture/smp-ipi-rate-limiting.md §0).
//!   - The initiator may hold the BKL while waiting for acks — sanctioned
//!     ONLY because responders ack lock-free from the S_SOFT trap (mailbox
//!     atomics + sfence, no BKL). Shootdowns are BKL-serialized, so there is
//!     never more than one initiator.
//!   - A responder spinning to ACQUIRE the BKL keeps interrupt windows open
//!     (sync::spin_irq acquisition contract), so it always acks.
//!   - Fail-closed: a hart not acking within the time budget is a lost-IPI
//!     kernel bug → panic (deterministic, never silent staleness).
//!     ADR: docs/rfcs/RFC-0021-kernel-smp-v1-percpu-runqueues-ipi-contract.md

use core::sync::atomic::{AtomicU64, Ordering};

use crate::types::CpuId;

use super::{cpu_current_id, cpu_online_mask, MAX_CPUS};

#[cfg(all(target_arch = "riscv64", target_os = "none"))]
use sbi_rt as sbi;

/// Global shootdown generation. Monotonic; one increment per shootdown.
static TLB_EPOCH: AtomicU64 = AtomicU64::new(0);

/// Per-hart mailbox: the epoch the hart must flush up to (`requested`) and
/// the epoch it has flushed (`acked`). v1 scope: every shootdown is a FULL
/// local flush on the responder (per-ASID scoping is a later optimization —
/// over-invalidation is always safe).
struct TlbMailbox {
    requested: AtomicU64,
    acked: AtomicU64,
}

static TLB_MAIL: [TlbMailbox; MAX_CPUS] =
    [const { TlbMailbox { requested: AtomicU64::new(0), acked: AtomicU64::new(0) } }; MAX_CPUS];

/// Wait budget for all acks: 100ms of mtime (10 MHz → 100ns/tick). A hart
/// that cannot ack within this is wedged or lost its IPI — fail closed.
#[cfg(all(target_arch = "riscv64", target_os = "none"))]
const ACK_BUDGET_TICKS: u64 = 1_000_000;

#[inline]
fn local_flush_all() {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    // SAFETY: full local TLB invalidation; over-invalidation is always safe.
    unsafe {
        core::arch::asm!("sfence.vma x0, x0", options(nostack, preserves_flags));
    }
}

/// Outcome of a responder mailbox poll (evidence for the counterfactual).
#[must_use = "shootdown poll outcomes feed the proof evidence"]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TlbPollOutcome {
    /// A requested epoch was pending: flushed and acked.
    Flushed,
    /// Nothing pending (counterfactual path).
    NoPending,
}

/// Responder side, called from the S_SOFT trap (LOCK-FREE — the initiator
/// may hold the BKL while waiting). Flushes and acks any pending epoch.
pub fn poll_mailbox(cpu: CpuId) -> TlbPollOutcome {
    let idx = cpu.as_index();
    if idx >= MAX_CPUS {
        return TlbPollOutcome::NoPending;
    }
    let requested = TLB_MAIL[idx].requested.load(Ordering::Acquire);
    if TLB_MAIL[idx].acked.load(Ordering::Acquire) >= requested {
        return TlbPollOutcome::NoPending;
    }
    local_flush_all();
    TLB_MAIL[idx].acked.store(requested, Ordering::Release);
    TlbPollOutcome::Flushed
}

/// Initiator side: flushes locally, advances the epoch, requests a flush from
/// every OTHER online hart (correctness IPI, unthrottled) and waits (bounded)
/// for all acks. Callers typically hold the BKL (mm paths) — see the module
/// invariants for why that is sanctioned.
pub fn shootdown_all() {
    local_flush_all();

    let me = cpu_current_id().as_index();
    let online = cpu_online_mask();
    let epoch = TLB_EPOCH.fetch_add(1, Ordering::AcqRel).wrapping_add(1);

    let mut targets = 0usize;
    for (idx, mail) in TLB_MAIL.iter().enumerate() {
        if idx == me || online & (1 << idx) == 0 {
            continue;
        }
        targets |= 1 << idx;
        // Monotonic max: coalesce with any (impossible today: BKL-serialized)
        // concurrent request.
        mail.requested.fetch_max(epoch, Ordering::AcqRel);
    }
    if targets == 0 {
        return;
    }

    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        // Correctness-class IPI: direct send, no rate limiting.
        let _ = sbi::send_ipi(targets, 0);

        let deadline = (riscv::register::time::read() as u64).saturating_add(ACK_BUDGET_TICKS);
        loop {
            let mut all_acked = true;
            for (idx, mail) in TLB_MAIL.iter().enumerate() {
                if targets & (1 << idx) == 0 {
                    continue;
                }
                if mail.acked.load(Ordering::Acquire) < epoch {
                    all_acked = false;
                    break;
                }
            }
            if all_acked {
                return;
            }
            if (riscv::register::time::read() as u64) >= deadline {
                // Fail closed: silent TLB staleness is never acceptable.
                panic!("tlb shootdown ack timeout");
            }
            core::hint::spin_loop();
        }
    }
}

/// Selftest evidence: `(epoch, requested[cpu], acked[cpu])`.
pub fn selftest_evidence(cpu: CpuId) -> (u64, u64, u64) {
    let idx = cpu.as_index();
    if idx >= MAX_CPUS {
        return (TLB_EPOCH.load(Ordering::Acquire), 0, 0);
    }
    (
        TLB_EPOCH.load(Ordering::Acquire),
        TLB_MAIL[idx].requested.load(Ordering::Acquire),
        TLB_MAIL[idx].acked.load(Ordering::Acquire),
    )
}
