// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: External-interrupt (PLIC) → userspace-endpoint routing. Lets a
//! userspace driver block on a device IRQ instead of polling: the kernel claims
//! the IRQ, delivers a notification to the bound endpoint, wakes the waiter, and
//! leaves the source masked at the PLIC until the driver services the device and
//! calls `irq_complete`. This is the reactive-input foundation (hidrawd blocks on
//! the virtio-input IRQ).
//! OWNERS: @kernel-hal-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! INVARIANTS: no allocation beyond a small notification Vec; a claimed IRQ stays
//!   masked until completed (level-triggered storm-safe); delivery runs only with
//!   exclusive borrows (U-mode S_EXT trap, a syscall, or the idle loop under the
//!   BKL) — an S-mode-interrupted hart claims into its per-hart stash instead and
//!   its next `dispatch_external` delivers (`stash_undelivered`).

extern crate alloc;

use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use crate::hal::plic::{self, IrqId, MAX_IRQ};
use crate::ipc::{self, EndpointId};
use crate::sched::Scheduler;
use crate::task;

/// IPC op byte for an IRQ-fired notification (payload: 4-byte LE source id).
pub const OP_IRQ_FIRED: u8 = 0x70;

const TABLE_LEN: usize = (MAX_IRQ as usize) + 1;

/// `irq source id -> bound endpoint id` (0 = unbound). Index 0 is unused (the
/// PLIC "no interrupt" sentinel). Atomics so a bind from a syscall is visible to
/// the S_EXT trap handler without a lock.
static IRQ_ENDPOINT: [AtomicU32; TABLE_LEN] = {
    const UNBOUND: AtomicU32 = AtomicU32::new(0);
    [UNBOUND; TABLE_LEN]
};

/// `irq source id -> target CPU index` for the binding (A6). v1 policy: all
/// bindings target the boot hart (its context takes the S_EXT trap); Phase B
/// routes per affinity. Recorded so completion targets the claiming context.
static IRQ_HOME_CPU: [AtomicU32; TABLE_LEN] = {
    const BOOT: AtomicU32 = AtomicU32::new(0);
    [BOOT; TABLE_LEN]
};

fn binding_cpu(irq: IrqId) -> crate::types::CpuId {
    crate::types::CpuId::from_raw(IRQ_HOME_CPU[irq.raw() as usize].load(Ordering::Acquire) as u16)
}

/// Binds `irq` to `endpoint` and enables the source at the PLIC for the boot
/// hart's context (v1 routing policy). Idempotent; a later bind re-points the
/// source.
pub fn bind(irq: IrqId, endpoint: EndpointId) {
    let cpu = crate::types::CpuId::BOOT;
    IRQ_ENDPOINT[irq.raw() as usize].store(endpoint, Ordering::Release);
    IRQ_HOME_CPU[irq.raw() as usize].store(cpu.as_index() as u32, Ordering::Release);
    plic::enable_source(irq, cpu);
}

/// Returns the endpoint bound to `irq`, if any.
#[must_use]
pub fn binding(irq: IrqId) -> Option<EndpointId> {
    match IRQ_ENDPOINT[irq.raw() as usize].load(Ordering::Acquire) {
        0 => None,
        ep => Some(ep),
    }
}

/// Completes `irq` at the PLIC so it can fire again. A driver calls this after it
/// has cleared the device's interrupt condition (drained the virtqueue). The
/// completion targets the BINDING's context — the syscall may run on any hart,
/// but the claim happened where the source is enabled (A6).
pub fn complete(irq: IrqId) {
    plic::complete(irq, binding_cpu(irq));
}

fn irq_payload(irq: IrqId) -> [u8; 4] {
    irq.raw().to_le_bytes()
}

/// Per-hart stash of sources claimed by an S-mode-interrupted hart (its
/// scheduler idle loop, where this very hart may hold the BKL, so delivery
/// cannot run inside the trap). A claimed source stays MASKED at the PLIC —
/// no level storm — and this hart's next `dispatch_external` (idle loop /
/// timer backstop / U-mode S_EXT) delivers it. Bitmask over the source ids
/// (`MAX_IRQ` = 95 fits two words).
///
/// Why not `drain_undelivered` once the runtime runs: completing a bound level
/// source without delivery re-asserts it immediately, so a hart that is
/// legitimately idle in S-mode (every blocking syscall now parks the hart
/// instead of spinning in U-mode — `park_hart_or_self_wake`) took the S_EXT
/// trap back-to-back and never reached its idle loop's `dispatch_external`:
/// the block-plane IRQ was never delivered (`init: volume spawn FAIL
/// reason=query`, 2026-09-08).
const STASH_WORDS: usize = (TABLE_LEN + 63) / 64;
static STASHED: [[AtomicU64; STASH_WORDS]; crate::smp::MAX_CPUS] = {
    const WORD: AtomicU64 = AtomicU64::new(0);
    const ROW: [AtomicU64; STASH_WORDS] = [WORD; STASH_WORDS];
    [ROW; crate::smp::MAX_CPUS]
};

/// Claims every pending source for this hart's context WITHOUT completing it
/// and records it in the hart's stash (bound sources) or quarantines it
/// (unbound: masked + completed). Used on the S-mode external trap once the
/// runtime runs; the idle loop's `dispatch_external` delivers the stash.
pub fn stash_undelivered() {
    let cpu = crate::smp::cpu_current_id().as_index();
    if cpu >= crate::smp::MAX_CPUS {
        drain_undelivered();
        return;
    }
    for _ in 0..TABLE_LEN {
        let Some(irq) = plic::claim() else {
            break;
        };
        let id = irq.raw() as usize;
        if binding(irq).is_some() && id < TABLE_LEN {
            STASHED[cpu][id / 64].fetch_or(1u64 << (id % 64), Ordering::AcqRel);
        } else {
            plic::disable_source(irq, crate::smp::cpu_current_id());
            plic::complete_current(irq);
        }
    }
}

/// Takes this hart's stash (claimed, undelivered, still masked sources).
fn take_stashed(cpu: usize) -> [u64; STASH_WORDS] {
    let mut out = [0u64; STASH_WORDS];
    if cpu < crate::smp::MAX_CPUS {
        for (w, slot) in out.iter_mut().enumerate() {
            *slot = STASHED[cpu][w].swap(0, Ordering::AcqRel);
        }
    }
    out
}

/// Claims and immediately completes every pending source without delivery, used
/// before the runtime is installed so a stray assertion cannot storm. Does NOT
/// mask bound sources (delivery resumes on the next U-mode trap). Bounded by the
/// source count.
pub fn drain_undelivered() {
    for _ in 0..TABLE_LEN {
        let Some(irq) = plic::claim() else {
            break;
        };
        // Unbound sources are masked so they cannot re-assert into a storm; bound
        // sources are left enabled (just completed) so a later U-mode trap can
        // deliver them.
        if binding(irq).is_none() {
            plic::disable_source(irq, crate::smp::cpu_current_id());
        }
        plic::complete_current(irq);
    }
}

/// Drains all pending external interrupts for our context, delivering each to its
/// bound endpoint (and waking a blocked driver). Unbound sources are quarantined
/// (masked + completed) so a stray device cannot storm the CPU.
///
/// Caller contract: invoked with exclusive borrows — the S_EXT/timer trap with
/// a USER-mode interrupted context, or the scheduler idle loop under the BKL —
/// so `router`/`tasks`/`scheduler` are the unique live borrows. Delivers this
/// hart's stash (sources already claimed by an S-mode trap) first.
pub fn dispatch_external(
    router: &mut ipc::Router,
    tasks: &mut task::TaskTable,
    scheduler: &mut Scheduler,
) {
    let cpu = crate::smp::cpu_current_id().as_index();
    let stashed = take_stashed(cpu);
    for (w, word) in stashed.iter().enumerate() {
        let mut bits = *word;
        while bits != 0 {
            let bit = bits.trailing_zeros() as usize;
            bits &= bits - 1;
            let id = w * 64 + bit;
            if id == 0 || id > MAX_IRQ as usize {
                continue;
            }
            let Some(irq) = IrqId::new(id as u32) else {
                continue;
            };
            match binding(irq) {
                Some(ep) => deliver(irq, ep, router, tasks, scheduler),
                None => {
                    // Unbound since the claim: quarantine like a fresh claim.
                    plic::disable_source(irq, crate::smp::cpu_current_id());
                    plic::complete_current(irq);
                }
            }
        }
    }
    // Bound: at most MAX_IRQ claims (each claimed source is masked until
    // completed, so the loop cannot spin on a re-asserting level IRQ).
    for _ in 0..TABLE_LEN {
        let Some(irq) = plic::claim() else {
            break;
        };
        match binding(irq) {
            Some(ep) => deliver(irq, ep, router, tasks, scheduler),
            None => {
                // No driver: mask + complete so it cannot re-fire indefinitely.
                plic::disable_source(irq, crate::smp::cpu_current_id());
                plic::complete_current(irq);
            }
        }
    }
}

/// Delivers one claimed source to its bound endpoint and wakes the waiter.
/// The source stays masked (no complete) until the driver services the device
/// and calls `irq_complete` — prevents a level storm.
fn deliver(
    irq: IrqId,
    ep: EndpointId,
    router: &mut ipc::Router,
    tasks: &mut task::TaskTable,
    scheduler: &mut Scheduler,
) {
    let payload = irq_payload(irq);
    let header =
        ipc::header::MessageHeader::new(0, ep, OP_IRQ_FIRED as u16, 0, payload.len() as u32);
    let msg = ipc::Message::new(header, alloc::vec::Vec::from(payload), None);
    if router.send(ep, msg).is_ok() {
        if let Ok(Some(waiter)) = router.pop_recv_waiter(ep) {
            let _ = tasks.wake(task::Pid::from_raw(waiter), scheduler);
        }
    }
}
