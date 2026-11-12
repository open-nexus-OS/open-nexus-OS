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

use core::sync::atomic::{AtomicU64, AtomicUsize};

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
    /// The last work-steal attempt (rate gate).
    pub(super) last_steal_ns: AtomicU64,
    /// TLB mailbox: the epoch this CPU must flush up to.
    pub(super) tlb_requested: AtomicU64,
    /// TLB mailbox: the epoch this CPU has flushed.
    pub(super) tlb_acked: AtomicU64,
    /// What this CPU was doing at its last tag point (evidence, not control).
    pub(super) activity: AtomicUsize,
    /// This CPU entered its scheduler loop.
    pub(crate) sched_loop_entered: AtomicUsize,
    /// The boot hart reported this CPU's first user dispatch.
    pub(crate) exec_proof_emitted: AtomicUsize,
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
            last_steal_ns: AtomicU64::new(0),
            tlb_requested: AtomicU64::new(0),
            tlb_acked: AtomicU64::new(0),
            activity: AtomicUsize::new(0),
            sched_loop_entered: AtomicUsize::new(0),
            exec_proof_emitted: AtomicUsize::new(0),
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
