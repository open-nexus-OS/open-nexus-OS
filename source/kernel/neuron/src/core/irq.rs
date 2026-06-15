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
//!   masked until completed (level-triggered storm-safe); accessed only from the
//!   boot hart with exclusive borrows (U-mode S_EXT trap or a syscall).

extern crate alloc;

use core::sync::atomic::{AtomicU32, Ordering};

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

/// Binds `irq` to `endpoint` and enables the source at the PLIC. Idempotent;
/// a later bind re-points the source.
pub fn bind(irq: IrqId, endpoint: EndpointId) {
    IRQ_ENDPOINT[irq.raw() as usize].store(endpoint, Ordering::Release);
    plic::enable_source(irq);
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
/// has cleared the device's interrupt condition (drained the virtqueue).
pub fn complete(irq: IrqId) {
    plic::complete(irq);
}

fn irq_payload(irq: IrqId) -> [u8; 4] {
    irq.raw().to_le_bytes()
}

/// Claims and immediately completes every pending source without delivery, used
/// on the rare S-mode external trap or before the runtime is installed so a
/// stray assertion cannot storm. Does NOT mask bound sources (delivery resumes on
/// the next U-mode trap). Bounded by the source count.
pub fn drain_undelivered() {
    for _ in 0..TABLE_LEN {
        let Some(irq) = plic::claim() else {
            break;
        };
        // Unbound sources are masked so they cannot re-assert into a storm; bound
        // sources are left enabled (just completed) so a later U-mode trap can
        // deliver them.
        if binding(irq).is_none() {
            plic::disable_source(irq);
        }
        plic::complete(irq);
    }
}

/// Drains all pending external interrupts for our context, delivering each to its
/// bound endpoint (and waking a blocked driver). Unbound sources are quarantined
/// (masked + completed) so a stray device cannot storm the CPU.
///
/// Caller contract: invoked from the S_EXT trap with a USER-mode interrupted
/// context on the boot hart, so `router`/`tasks`/`scheduler` are the unique live
/// borrows (mirrors the timer path).
pub fn dispatch_external(
    router: &mut ipc::Router,
    tasks: &mut task::TaskTable,
    scheduler: &mut Scheduler,
) {
    // Bound: at most MAX_IRQ claims (each claimed source is masked until
    // completed, so the loop cannot spin on a re-asserting level IRQ).
    for _ in 0..TABLE_LEN {
        let Some(irq) = plic::claim() else {
            break;
        };
        match binding(irq) {
            Some(ep) => {
                let payload = irq_payload(irq);
                let header = ipc::header::MessageHeader::new(
                    0,
                    ep,
                    OP_IRQ_FIRED as u16,
                    0,
                    payload.len() as u32,
                );
                let msg = ipc::Message::new(header, alloc::vec::Vec::from(payload), None);
                // Leave the source masked (no complete) until the driver services
                // the device and calls irq_complete — prevents a level storm.
                if router.send(ep, msg).is_ok() {
                    if let Ok(Some(waiter)) = router.pop_recv_waiter(ep) {
                        let _ = tasks.wake(task::Pid::from_raw(waiter), scheduler);
                    }
                }
            }
            None => {
                // No driver: mask + complete so it cannot re-fire indefinitely.
                plic::disable_source(irq);
                plic::complete(irq);
            }
        }
    }
}
