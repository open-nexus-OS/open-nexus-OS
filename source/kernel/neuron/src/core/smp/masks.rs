// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Per-hart state masks beside the online mask: the idle mask that
//! lets a TLB shootdown skip harts parked in WFI (they flush at their next
//! user dispatch), plus the online predicate. Split out of `smp/mod.rs`
//! (module-size ratchet).
//! OWNERS: @kernel
//! STATUS: Experimental
//! TEST_COVERAGE: QEMU SMP proofs (`KSELFTEST: tlb shootdown ok`), interactive 4-CPU boots

use core::sync::atomic::{AtomicUsize, Ordering};

use crate::types::CpuId;

use super::cpu_online_mask;

/// Harts parked in the scheduler's WFI idle (no user translation in use).
/// A TLB shootdown does not wait for them: they flush at their next user
/// dispatch (`tlb::poll_mailbox` before `context_switch_to_task`), so a
/// lost/late IPI to a sleeping hart can never turn into an ack timeout.
static CPU_IDLE_MASK: AtomicUsize = AtomicUsize::new(0);

/// Marks `cpu` idle (parked in WFI, nothing dispatched) or busy again.
#[inline]
pub fn mark_cpu_idle(cpu: CpuId, idle: bool) {
    let bit = 1usize << cpu.as_index();
    if idle {
        CPU_IDLE_MASK.fetch_or(bit, Ordering::AcqRel);
    } else {
        CPU_IDLE_MASK.fetch_and(!bit, Ordering::AcqRel);
    }
}

/// Harts currently parked idle (see [`mark_cpu_idle`]).
#[inline]
pub fn cpu_idle_mask() -> usize {
    CPU_IDLE_MASK.load(Ordering::Acquire)
}

#[inline]
pub fn cpu_is_online(cpu: CpuId) -> bool {
    let bit = 1usize << cpu.as_index();
    cpu_online_mask() & bit != 0
}
