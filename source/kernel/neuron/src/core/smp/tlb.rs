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
//!   - Responders ack lock-free from the S_SOFT trap (mailbox atomics +
//!     sfence, no BKL). The vm_unmap SYSCALL initiates with the BKL DROPPED
//!     (phased; a BKL-held ack wait tripped the 10ms budget gate at SMP≥2);
//!     concurrent initiators are safe by construction — the epoch fetch_add
//!     plus per-mailbox `fetch_max` coalesce, and an ack for a later epoch
//!     satisfies every earlier one.
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

/// Per-hart activity tag (evidence, not control): what a hart was doing
/// when it last passed a tag point. Named on the `KPANIC: tlb shootdown ack
/// timeout` line so a hart that never acked is not a blank — the 2026-09-08
/// timeouts could only be *inferred* to be a phase-B memset.
static HART_ACTIVITY: [core::sync::atomic::AtomicUsize; MAX_CPUS] =
    [const { core::sync::atomic::AtomicUsize::new(0) }; MAX_CPUS];

/// Activity tags (`HART_ACTIVITY`): low byte = class, upper bits = detail
/// (syscall number / scause).
pub const ACT_TRAP: usize = 1;
pub const ACT_PHASE_B_ZERO: usize = 2;
pub const ACT_PHASE_B_COPY: usize = 3;
pub const ACT_IDLE_LOOP: usize = 4;
/// User-fault kill path stages: 0 dumped, 1 BKL acquired, 2 task released.
pub const ACT_FAULT_KILL: usize = 5;

#[inline]
pub fn note_activity(cpu: CpuId, class: usize, detail: usize) {
    let idx = cpu.as_index();
    if idx < MAX_CPUS {
        HART_ACTIVITY[idx].store((detail << 8) | (class & 0xff), Ordering::Relaxed);
    }
}

/// Chunk size for unlocked phase-B byte moves: 64 KiB is ~100 µs of memset
/// under TCG, so a shootdown request waits at most that long per chunk.
const UNLOCKED_CHUNK: usize = 64 * 1024;

/// Phase-B `write_bytes` (BKL dropped, SIE=0 in trap context): a shootdown
/// IPI cannot interrupt the memset, so a large VMO (a 4 MiB framebuffer)
/// parked the requester for its whole duration and, with TCG scheduling
/// jitter, past the ack budget — `tlb shootdown ack timeout` with the
/// silent hart mid-zero (2026-09-08). This hart holds no kernel state in
/// phase B, so it services its mailbox between chunks.
///
/// # Safety
/// Same contract as `core::ptr::write_bytes(dst, 0, len)`.
pub unsafe fn zero_bytes_polled(dst: *mut u8, len: usize) {
    let me = cpu_current_id();
    note_activity(me, ACT_PHASE_B_ZERO, len);
    let mut off = 0;
    while off < len {
        let n = core::cmp::min(UNLOCKED_CHUNK, len - off);
        // SAFETY: sub-range of the caller's contract.
        unsafe { core::ptr::write_bytes(dst.add(off), 0, n) };
        off += n;
        let _ = poll_mailbox(me);
    }
}

/// Phase-B `copy_nonoverlapping` with the same chunked mailbox service as
/// `zero_bytes_polled` (ELF images are up to a few MiB).
///
/// # Safety
/// Same contract as `core::ptr::copy_nonoverlapping(src, dst, len)`.
pub unsafe fn copy_bytes_polled(src: *const u8, dst: *mut u8, len: usize) {
    let me = cpu_current_id();
    note_activity(me, ACT_PHASE_B_COPY, len);
    let mut off = 0;
    while off < len {
        let n = core::cmp::min(UNLOCKED_CHUNK, len - off);
        // SAFETY: sub-range of the caller's contract.
        unsafe { core::ptr::copy_nonoverlapping(src.add(off), dst.add(off), n) };
        off += n;
        let _ = poll_mailbox(me);
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
        // Monotonic max: coalesce with a concurrent request. Concurrent
        // initiators ARE possible: `vm_unmap` shoots down in phase B with the
        // BKL dropped while a task exit (AS destroy / ASID recycle) shoots
        // down under the BKL on another hart.
        mail.requested.fetch_max(epoch, Ordering::AcqRel);
    }
    if targets == 0 {
        return;
    }
    // Idle harts (parked in WFI, no user translation live) flush at their
    // next dispatch instead of acking now: read AFTER the mailboxes were
    // raised, so a hart leaving idle from here on sees the request at its
    // dispatch poll. The doorbell still goes to every target.
    let wait_targets = targets & !super::cpu_idle_mask();
    if wait_targets == 0 {
        #[cfg(all(target_arch = "riscv64", target_os = "none"))]
        let _ = sbi::send_ipi(targets, 0);
        return;
    }

    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        // Bounded rounds, doorbell RE-SENT each round: an IPI is a level on
        // `sip` that any trap-side `clear_ssoft` can consume — the responder
        // handler now clears before reading the mailbox, but re-sending
        // keeps a consumed-doorbell class recoverable instead of fatal,
        // and gives TCG vCPU-scheduling jitter more than one 100ms window.
        const RESEND_ROUNDS: u32 = 4;
        for _ in 0..RESEND_ROUNDS {
            // Correctness-class IPI: direct send, no rate limiting. Only
            // still-missing harts get the re-send.
            let mut missing = 0usize;
            for (idx, mail) in TLB_MAIL.iter().enumerate() {
                if wait_targets & (1 << idx) != 0 && mail.acked.load(Ordering::Acquire) < epoch {
                    missing |= 1 << idx;
                }
            }
            if missing == 0 {
                return;
            }
            // Doorbell to every target (idle ones ack early when it lands).
            let _ = sbi::send_ipi(targets, 0);
            let deadline = (riscv::register::time::read() as u64).saturating_add(ACK_BUDGET_TICKS);
            while (riscv::register::time::read() as u64) < deadline {
                let mut all_acked = true;
                for (idx, mail) in TLB_MAIL.iter().enumerate() {
                    if wait_targets & (1 << idx) != 0 && mail.acked.load(Ordering::Acquire) < epoch
                    {
                        all_acked = false;
                        break;
                    }
                }
                if all_acked {
                    return;
                }
                // A waiting initiator is also a responder: with two
                // concurrent initiators (see above) each waits for the
                // other's ack, and neither takes a trap while spinning
                // (BKL-held caller, or SIE=0 in a phase-B trap context) —
                // the `hart=2 … activity=class1:0xd10` timeout: hart 2 in
                // the fault-probe kill path (AS destroy shootdown) vs cpu0
                // in a phase-B `vm_unmap` shootdown (2026-09-08).
                let _ = poll_mailbox(cpu_current_id());
                core::hint::spin_loop();
            }
        }
        // Fail closed: silent TLB staleness is never acceptable. Name the
        // evidence — WHICH hart, and how far its mailbox got.
        for (idx, mail) in TLB_MAIL.iter().enumerate() {
            if wait_targets & (1 << idx) != 0 && mail.acked.load(Ordering::Acquire) < epoch {
                // The panic path cannot format arguments; name the evidence
                // on the log first (which hart, how far its mailbox got).
                let act = HART_ACTIVITY[idx].load(Ordering::Relaxed);
                log_error!(
                    target: "smp",
                    "KPANIC: tlb shootdown ack timeout hart={} epoch={} requested={} acked={} online=0x{:x} me={} activity=class{}:0x{:x}",
                    idx,
                    epoch,
                    mail.requested.load(Ordering::Acquire),
                    mail.acked.load(Ordering::Acquire),
                    online,
                    me,
                    act & 0xff,
                    act >> 8
                );
                panic!(
                    "tlb shootdown ack timeout hart={} epoch={} requested={} acked={}",
                    idx,
                    epoch,
                    mail.requested.load(Ordering::Acquire),
                    mail.acked.load(Ordering::Acquire)
                );
            }
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
