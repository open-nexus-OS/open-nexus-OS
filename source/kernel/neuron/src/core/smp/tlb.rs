// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: A5 TLB shootdown — epoch-based, allocation-free, deterministic. An unmap in one
//! address space reaches only the harts that may hold a translation of it (TASK-0330 P3,
//! `shootdown_of`; the set is `hart_spaces::HartSpaces`'), the recycling of the ASIDs every
//! hart (`shootdown_all`).
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU SMP proofs (tlb shootdown ok / counterfactual / targeted / skipped);
//!   the sets on the host (`hart_spaces`)
//! PUBLIC API: shootdown_all(), shootdown_of(), poll_mailbox(), selftest evidence accessors
//! INVARIANTS:
//!   - A hart that may hold a translation of a space is in that space's set before it can
//!     load one, and leaves it only by its own full flush while it runs on another space.
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

use nexus_sched::CpuSet;

use crate::types::CpuId;

use super::{cpu_current_id, cpu_online, hart_state};

/// Global shootdown generation. Monotonic; one increment per shootdown.
static TLB_EPOCH: AtomicU64 = AtomicU64::new(0);

// Per-hart mailbox (`hart_state`: `tlb_requested`, `tlb_acked`): the epoch the hart must
// flush up to and the epoch it has flushed. v1 scope: every shootdown is a FULL local flush
// on the responder (per-ASID scoping is a later optimization — over-invalidation is always
// safe).

/// The CPUs of `waited` whose mailbox has not reached `epoch`.
fn not_acked(waited: CpuSet, epoch: u64) -> CpuSet {
    hart_state::all()
        .filter(|(cpu, hart)| {
            waited.contains(*cpu) && hart.tlb_acked.load(Ordering::Acquire) < epoch
        })
        .fold(CpuSet::EMPTY, |missing, (cpu, _)| missing.with(cpu))
}

/// Wait budget for all acks: 100 ms of the platform's timebase (was the literal
/// `1_000_000` ticks — 100 ms only at QEMU's 10 MHz). A hart that cannot ack
/// within this is wedged or lost its IPI — fail closed.
#[cfg(all(target_arch = "riscv64", target_os = "none"))]
fn ack_budget_ticks() -> u64 {
    crate::hal::platform::ns_to_ticks(100_000_000)
}

#[inline]
fn local_flush_all() {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    // SAFETY: full local TLB invalidation; over-invalidation is always safe.
    unsafe {
        core::arch::asm!("sfence.vma x0, x0", options(nostack, preserves_flags));
    }
}

/// Activity tags — per hart (`hart_state`: `activity`), evidence, not control: what a hart
/// was doing when it last passed a tag point. Named on the `KPANIC: tlb shootdown ack
/// timeout` line so a hart that never acked is not a blank — the 2026-09-08 timeouts could
/// only be *inferred* to be a phase-B memset. Low byte = class, upper bits = detail
/// (syscall number / scause).
pub const ACT_TRAP: usize = 1;
pub const ACT_PHASE_B_ZERO: usize = 2;
pub const ACT_PHASE_B_COPY: usize = 3;
pub const ACT_IDLE_LOOP: usize = 4;
/// User-fault kill path stages: 0 dumped, 1 BKL acquired, 2 task released.
pub const ACT_FAULT_KILL: usize = 5;

/// Spinning for a lock (detail: the lock's address without its top byte) — until the take.
pub const ACT_SPIN_WAIT: usize = 6;
/// Waiting for shootdown acks (detail: the epoch) — until every waited hart acked.
pub const ACT_TLB_WAIT: usize = 7;
/// Waiting for the kernel lock (detail: what for — `lock_share`'s holder id) — until the take.
pub const ACT_LOCK_WAIT: usize = 8;
/// The idle loop under the kernel lock (detail: 1 taken, 2 picked or stolen, 3 dispatch,
/// 4 deadline walk, 5 events, 6 witness, 7 a stashed line, 8 its own deadline).
pub const ACT_IDLE_LOCKED: usize = 9;
/// Arming the timer (the firmware's call where the hart has no `stimecmp`).
pub const ACT_TIMER_ARM: usize = 10;
/// An unmap under the kernel lock (detail: 1 clear, 2 forget, 3 the object's reclaim).
pub const ACT_UNMAP: usize = 11;

#[inline]
pub fn note_activity(cpu: CpuId, class: usize, detail: usize) {
    if let Some(hart) = hart_state::of(cpu) {
        hart.activity.store((detail << 8) | (class & 0xff), Ordering::Relaxed);
    }
}

/// This hart's tag for the length of a wait: set here, the tag before handed back for
/// [`restore_activity`] when the wait ends — so a stall line (`KINIT: bkl stall`) reads what a
/// hart waits for, and the tag it had is not lost to the wait.
#[inline]
pub fn swap_activity(class: usize, detail: usize) -> usize {
    hart_state::of(cpu_current_id())
        .map_or(0, |hart| hart.activity.swap((detail << 8) | (class & 0xff), Ordering::Relaxed))
}

/// Ends a wait tagged by [`swap_activity`].
#[inline]
pub fn restore_activity(tag: usize) {
    if let Some(hart) = hart_state::of(cpu_current_id()) {
        hart.activity.store(tag, Ordering::Relaxed);
    }
}

/// CPU `cpu`'s tag as stored (low byte: class, above: detail); 0 for no such CPU.
pub fn activity_of(cpu: usize) -> usize {
    hart_state::at(cpu).map_or(0, |hart| hart.activity.load(Ordering::Relaxed))
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

/// The stretch between a translation and the last byte copied through it (ADR-0102): while
/// one lives, the executing hart takes no lock and orders no shootdown, so it acknowledges
/// none — and the frames and table pages it reads through are not freed under it.
pub struct NoAck(Option<&'static hart_state::HartState>);

impl NoAck {
    /// Begins the stretch on the executing hart (interrupts are off: a trap context).
    pub fn begin() -> Self {
        let hart = hart_state::of(cpu_current_id());
        if let Some(hart) = hart {
            hart.no_ack.store(1, Ordering::Relaxed);
        }
        Self(hart)
    }
}

impl Drop for NoAck {
    fn drop(&mut self) {
        if let Some(hart) = self.0 {
            hart.no_ack.store(0, Ordering::Relaxed);
        }
    }
}

/// Responder side, called from the S_SOFT trap (LOCK-FREE — the initiator
/// may hold the BKL while waiting). Flushes and acks any pending epoch.
pub fn poll_mailbox(cpu: CpuId) -> TlbPollOutcome {
    let Some(hart) = hart_state::of(cpu) else {
        return TlbPollOutcome::NoPending;
    };
    // ADR-0102: an unmap frees only after every acknowledgement — a hart inside a user copy
    // holds a translation and gives none (a lock's spin there is a bug, named here).
    assert!(hart.no_ack.load(Ordering::Relaxed) == 0, "shootdown polled inside a user copy");
    let requested = hart.tlb_requested.load(Ordering::Acquire);
    if hart.tlb_acked.load(Ordering::Acquire) >= requested {
        return TlbPollOutcome::NoPending;
    }
    local_flush_all();
    // The flush emptied this hart of every space's translations: of the one space the
    // requests were for it holds none any more unless it runs on it, and leaves its set.
    left_after_flush(cpu.as_index(), hart.tlb_space.swap(0, Ordering::AcqRel));
    hart.tlb_acked.store(requested, Ordering::Release);
    TlbPollOutcome::Flushed
}

/// A mailbox whose pending requests are for several spaces, or for every hart.
pub(super) const MIXED: u32 = u32::MAX;

fn left_after_flush(cpu: usize, space: u32) {
    if space != 0 && space != MIXED {
        if let Some(harts) = crate::hart_spaces::installed() {
            harts.flushed(cpu, space);
        }
    }
}

/// Shootdowns of every hart, shootdowns for one space, and the harts those reached.
static ALL: AtomicU64 = AtomicU64::new(0);
static TARGETED: AtomicU64 = AtomicU64::new(0);
static REACHED: AtomicU64 = AtomicU64::new(0);

/// `(shootdowns of every hart, shootdowns for one space, harts those reached)` since boot.
pub fn counts() -> (u64, u64, u64) {
    let read = |counter: &AtomicU64| counter.load(Ordering::Relaxed);
    (read(&ALL), read(&TARGETED), read(&REACHED))
}

/// Initiator side: flushes locally, advances the epoch, requests a flush from
/// every OTHER online hart (correctness IPI, unthrottled) and waits (bounded)
/// for all acks. Callers typically hold the BKL (mm paths) — see the module
/// invariants for why that is sanctioned.
pub fn shootdown_all() {
    ALL.fetch_add(1, Ordering::Relaxed);
    let _ = shootdown(None);
}

/// [`shootdown_all`] for the one address space `raw` (TASK-0330 P3): only the harts that may
/// hold a translation of it are reached (`HartSpaces::reads_through`) — the others never loaded
/// one, or flushed since. A hart reached while it runs on another space leaves the space's
/// set. Answers the CPUs reached (the selftest's evidence). A space the table does not know
/// (`raw` 0: not placed yet) reaches every hart.
pub fn shootdown_of(raw: u32) -> CpuSet {
    let known = crate::hart_spaces::installed().is_some_and(|harts| harts.ran(raw).is_some());
    if !known {
        shootdown_all();
        return cpu_online().without(CpuSet::EMPTY.with(cpu_current_id().as_index()));
    }
    let reached = shootdown(Some(raw));
    TARGETED.fetch_add(1, Ordering::Relaxed);
    REACHED.fetch_add(reached.count() as u64, Ordering::Relaxed);
    reached
}

/// The body of both: `space` is the one space the cleared entries belong to, or `None`.
fn shootdown(space: Option<u32>) -> CpuSet {
    local_flush_all();

    let me = cpu_current_id().as_index();
    let online = cpu_online();
    // The caller cleared its entries before this point. A hart that enters the space's set
    // from here on set its bit, then read the tables: with a full barrier on both sides it
    // reads the cleared entries, or its bit is read here.
    core::sync::atomic::fence(Ordering::SeqCst);
    let (reach, names) = match space.zip(crate::hart_spaces::installed()) {
        Some((raw, harts)) => {
            // This hart flushed too: off the space, it holds nothing of it either.
            harts.flushed(me, raw);
            (harts.ran(raw).map_or(online, CpuSet::from_bits), raw)
        }
        None => (online, MIXED),
    };
    let epoch = TLB_EPOCH.fetch_add(1, Ordering::AcqRel).wrapping_add(1);

    let mut targets = CpuSet::EMPTY;
    for (cpu, hart) in hart_state::all() {
        if cpu == me || !online.contains(cpu) || !reach.contains(cpu) {
            continue;
        }
        targets = targets.with(cpu);
        // The space first, then the epoch: a hart that sees the request sees what it is for.
        // Requests for two spaces pending at once name none (nobody leaves a set on them).
        let _ = hart.tlb_space.fetch_update(Ordering::AcqRel, Ordering::Acquire, |pending| {
            Some(if pending == 0 || pending == names { names } else { MIXED })
        });
        // Monotonic max: coalesce with a concurrent request. Concurrent
        // initiators ARE possible: `vm_unmap` shoots down in phase B with the
        // BKL dropped while a task exit (AS destroy / ASID recycle) shoots
        // down under the BKL on another hart.
        hart.tlb_requested.fetch_max(epoch, Ordering::AcqRel);
    }
    if targets.is_empty() {
        return targets;
    }
    // Idle harts (parked in WFI, no user translation live) flush at their
    // next dispatch instead of acking now: read AFTER the mailboxes were
    // raised, so a hart leaving idle from here on sees the request at its
    // dispatch poll. The doorbell still goes to every target.
    let wait_targets = targets.without(super::cpu_idle());
    if wait_targets.is_empty() {
        #[cfg(all(target_arch = "riscv64", target_os = "none"))]
        let _ = super::send_ipi(targets);
        return targets;
    }

    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        // Bounded rounds, doorbell RE-SENT each round: an IPI is a level on
        // `sip` that any trap-side `clear_ssoft` can consume — the responder
        // handler now clears before reading the mailbox, but re-sending
        // keeps a consumed-doorbell class recoverable instead of fatal,
        // and gives TCG vCPU-scheduling jitter more than one 100ms window.
        const RESEND_ROUNDS: u32 = 4;
        let before = swap_activity(ACT_TLB_WAIT, epoch as usize);
        for _ in 0..RESEND_ROUNDS {
            // Correctness-class IPI: direct send, no rate limiting.
            if not_acked(wait_targets, epoch).is_empty() {
                restore_activity(before);
                return targets;
            }
            // Doorbell to every target (idle ones ack early when it lands).
            let sent = super::send_ipi(targets);
            let deadline =
                (riscv::register::time::read() as u64).saturating_add(ack_budget_ticks());
            while (riscv::register::time::read() as u64) < deadline {
                if not_acked(wait_targets, epoch).is_empty() {
                    restore_activity(before);
                    return targets;
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
            note_slow_ack(not_acked(wait_targets, epoch), epoch, sent);
        }
        // Fail closed: silent TLB staleness is never acceptable. Name the
        // evidence — WHICH hart, and how far its mailbox got.
        if let Some(cpu) = not_acked(wait_targets, epoch).lowest() {
            let (_, requested, acked) = selftest_evidence(CpuId::from_raw(cpu as u16));
            // The panic path cannot format arguments; name the evidence
            // on the log first (which CPU, how far its mailbox got).
            let act = hart_state::at(cpu).map_or(0, |hart| hart.activity.load(Ordering::Relaxed));
            log_error!(
                target: "smp",
                "KPANIC: tlb shootdown ack timeout hart={} epoch={} requested={} acked={} online=0x{:x} me={} activity=class{}:0x{:x}",
                cpu,
                epoch,
                requested,
                acked,
                online,
                me,
                act & 0xff,
                act >> 8
            );
            panic!(
                "tlb shootdown ack timeout hart={} epoch={} requested={} acked={}",
                cpu, epoch, requested, acked
            );
        }
    }
    targets
}

/// A round of the wait ended without every acknowledgement: the hart it waited for and what
/// that hart was doing, named for the first few — a round is 100 ms of everyone who shares a
/// lock the initiator holds.
#[cfg(all(target_arch = "riscv64", target_os = "none"))]
fn note_slow_ack(late: CpuSet, epoch: u64, sent: bool) {
    static LOGGED: AtomicU64 = AtomicU64::new(0);
    let Some(cpu) = late.lowest() else { return };
    if LOGGED.fetch_add(1, Ordering::Relaxed) >= 4 {
        return;
    }
    let Some(hart) = hart_state::at(cpu) else { return };
    let act = hart.activity.load(Ordering::Relaxed);
    log_info!(
        target: "smp",
        "KINIT: tlb slow ack hart={} epoch={} me={} sent={} soft_traps={} in_loop={} idle=0x{:x} activity=class{}:0x{:x}",
        cpu,
        epoch,
        cpu_current_id().as_index(),
        sent,
        hart.resched_ssoft_traps.load(Ordering::Relaxed),
        hart.sched_loop_entered.load(Ordering::Relaxed),
        super::cpu_idle().bits(),
        act & 0xff,
        act >> 8
    );
}

/// Selftest evidence: `(epoch, requested[cpu], acked[cpu])`.
pub fn selftest_evidence(cpu: CpuId) -> (u64, u64, u64) {
    let epoch = TLB_EPOCH.load(Ordering::Acquire);
    match hart_state::of(cpu) {
        Some(hart) => (
            epoch,
            hart.tlb_requested.load(Ordering::Acquire),
            hart.tlb_acked.load(Ordering::Acquire),
        ),
        None => (epoch, 0, 0),
    }
}
