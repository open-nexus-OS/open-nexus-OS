// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: what the SMP layer keeps for one logical CPU — reschedule requests and their
//! evidence, the wake hint, the tick and dispatch counters, the TLB mailbox, the activity tag,
//! the scheduler-loop flags. ONE record per CPU in ONE table sized by the machine (RFC-0118),
//! each record on its own cache lines: a counter one hart bumps never shares a line with the
//! one another hart bumps (the arrays of four words these fields were did exactly that).
//! Every field is an atomic: the owning hart writes most of them, another hart raises a
//! request or reads the evidence.
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: QEMU SMP proofs (`KSELFTEST: per-hart state ok`, the resched/TLB ladders);
//!   the table itself is host-tested in `sync::hart_table`
//! INVARIANTS: no entry is reached by a hart id; a CPU the machine does not have has no record
//! RFC: docs/rfcs/RFC-0118-n-harts-from-the-device-tree-a-64-bit-cpu-set-the-topology-exported.md

use core::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize};

use crate::sync::hart_table::{HartTable, SizeError};
use crate::types::CpuId;

/// The SMP layer's state of one logical CPU.
#[repr(align(64))]
pub(crate) struct HartState {
    /// A reschedule is outstanding (claimed by the requester, consumed by the target).
    pub(super) resched_pending: AtomicUsize,
    /// Requests accepted for this CPU.
    pub(super) resched_req_accepted: AtomicUsize,
    /// Wake interrupts the firmware accepted for this CPU.
    pub(super) resched_ipi_sent_ok: AtomicUsize,
    /// Wake interrupts elided because a reschedule was already outstanding.
    pub(super) resched_ipi_skipped: AtomicUsize,
    /// Supervisor software traps this CPU took.
    pub(super) resched_ssoft_traps: AtomicUsize,
    /// Requests this CPU acknowledged.
    pub(super) resched_ack: AtomicUsize,
    /// WFI wake hint: set by `request_resched`, consumed only by the target's idle path.
    pub(super) wake_hint: AtomicUsize,
    /// User-task dispatches this CPU performed.
    pub(super) user_dispatches: AtomicUsize,
    /// Supervisor timer ticks this CPU took.
    pub(super) timer_ticks: AtomicUsize,
    /// TASK-0363 P4: timer interrupts that woke this CPU in the kernel (idle), in the window, and
    /// of those the ones that found nothing due (a tick, not a deadline).
    pub(super) idle_timer_wakes: AtomicUsize,
    pub(super) idle_timer_empty: AtomicUsize,
    /// Of the empty ones those that were this CPU's safety net (by design, at most two a second),
    /// and when the net it armed falls due (ns; 0 = the compare register holds a deadline).
    pub(super) idle_timer_net: AtomicUsize,
    pub(super) idle_net_at_ns: AtomicU64,
    /// Of the stray ones those at a passed deadline (`record_idle_timer`).
    pub(super) idle_timer_stale: AtomicUsize,
    /// Idle walks that found a deadline due and woke nothing (TASK-0363 P8).
    pub(super) idle_timer_dry: AtomicUsize,
    /// Time asleep in the window (ns, TASK-0363 P5: the hart's busy share).
    pub(super) idle_sleep_ns: AtomicU64,
    /// The last work-steal attempt (rate gate).
    pub(super) last_steal_ns: AtomicU64,
    /// TLB mailbox: the epoch this CPU must flush up to.
    pub(super) tlb_requested: AtomicU64,
    /// TLB mailbox: the epoch this CPU has flushed.
    pub(super) tlb_acked: AtomicU64,
    /// TLB mailbox: the one space the pending requests are for (its raw handle), so a hart
    /// that is not on it can leave its set after the flush; 0 none, `tlb::MIXED` several
    /// spaces or every hart's shootdown.
    pub(super) tlb_space: AtomicU32,
    /// This CPU is between a translation and the last byte copied through it (`tlb::NoAck`,
    /// ADR-0102): it must not acknowledge a shootdown until the stretch ends.
    pub(super) no_ack: AtomicUsize,
    /// What this CPU was doing at its last tag point (evidence, not control).
    pub(super) activity: AtomicUsize,
    /// This CPU entered its scheduler loop.
    pub(crate) sched_loop_entered: AtomicUsize,
    /// The boot hart reported this CPU's first user dispatch.
    pub(crate) exec_proof_emitted: AtomicUsize,
    /// Where this CPU last entered a space and last stopped running a task (evidence).
    pub(super) trail: super::trail::Trail,
}

impl HartState {
    const fn new() -> Self {
        Self {
            resched_pending: AtomicUsize::new(0),
            resched_req_accepted: AtomicUsize::new(0),
            resched_ipi_sent_ok: AtomicUsize::new(0),
            resched_ipi_skipped: AtomicUsize::new(0),
            resched_ssoft_traps: AtomicUsize::new(0),
            resched_ack: AtomicUsize::new(0),
            wake_hint: AtomicUsize::new(0),
            user_dispatches: AtomicUsize::new(0),
            timer_ticks: AtomicUsize::new(0),
            idle_timer_wakes: AtomicUsize::new(0),
            idle_timer_empty: AtomicUsize::new(0),
            idle_timer_net: AtomicUsize::new(0),
            idle_net_at_ns: AtomicU64::new(0),
            idle_timer_stale: AtomicUsize::new(0),
            idle_timer_dry: AtomicUsize::new(0),
            idle_sleep_ns: AtomicU64::new(0),
            last_steal_ns: AtomicU64::new(0),
            tlb_requested: AtomicU64::new(0),
            tlb_acked: AtomicU64::new(0),
            tlb_space: AtomicU32::new(0),
            no_ack: AtomicUsize::new(0),
            activity: AtomicUsize::new(0),
            sched_loop_entered: AtomicUsize::new(0),
            exec_proof_emitted: AtomicUsize::new(0),
            trail: super::trail::Trail::new(),
        }
    }
}

static HARTS: HartTable<HartState> = HartTable::new(HartState::new());

/// The record of logical CPU `cpu`; `None` for a CPU the machine does not have.
#[inline]
pub(crate) fn of(cpu: CpuId) -> Option<&'static HartState> {
    HARTS.get(cpu.as_index())
}

/// The record of the CPU with index `cpu`.
#[inline]
pub(super) fn at(cpu: usize) -> Option<&'static HartState> {
    HARTS.get(cpu)
}

/// Every record with its CPU index, CPU 0 first.
pub(super) fn all() -> impl Iterator<Item = (usize, &'static HartState)> {
    HARTS.iter().enumerate()
}

/// Sizes the table for a machine of `harts` CPUs (the boot hart, once, before a secondary
/// starts).
pub(super) fn size(harts: usize) -> Result<(), SizeError> {
    HARTS.size(harts, HartState::new)
}

/// The boot gate of the per-hart state (every profile, after the secondaries ran their
/// first passes): the table holds exactly the machine's CPUs, and no hart ever asked a
/// per-hart table for an entry the sizing forgot (`hart_table::unsized_hits`).
pub(crate) fn selftest() {
    let harts = crate::topology::hart_count();
    let records = HARTS.len();
    let unsized_hits = crate::sync::hart_table::unsized_hits();
    let exact = records == harts && HARTS.get(harts.saturating_sub(1)).is_some();
    if exact && HARTS.get(harts).is_none() && unsized_hits == 0 {
        log_info!(target: "selftest", "KSELFTEST: per-hart state ok (harts={} unsized=0)", harts);
    } else {
        log_error!(
            target: "selftest",
            "KSELFTEST: per-hart state FAIL (harts={} records={} unsized={})",
            harts,
            records,
            unsized_hits
        );
    }
}
