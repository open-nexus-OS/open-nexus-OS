// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The control queue's bounded ring waits (TASK-0054C P2-b): `alloc_free_slot`
//! (back-pressure on a full ring) and `wait_slot` (a command's completion) wait on the GPU
//! ring-buffer IRQ beside the device watchdog — a kernel one-shot on gpud's declared
//! device-watchdog pair — and recover from a lost IRQ when the watchdog fires. No receive
//! deadline, no clock compare: the clock is read once to COMPUTE the bound the kernel timer
//! then owns. Split out of `virtqueue.rs` (structure gate); a child module so the queue's
//! fields stay private.
//! OWNERS: @gpu
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: QEMU (`gpud: gpu irq wake`, the present ladder; `gpud: present us …`
//!   counts watchdog expiries).

use super::{
    CtrlQueue, RingSlot, GPU_IRQ_WAKE_LOGGED, GPU_WAIT_DEADLINE_NS, IRQ_DEADLINE_EXPIRED_COUNT,
    IRQ_WAKE_COUNT,
};
use alloc::rc::Rc;
use core::cell::RefCell;
use nexus_gfx::backend::error::GfxError;
use nexus_ipc::timer::{NotifyTimer, Waitset};

/// The GPU ring-buffer wait's bound (TASK-0054C P2-b): a kernel one-shot on gpud's declared
/// device-watchdog pair, a waitset member beside the IRQ endpoint. A lost or late IRQ ends the
/// wait through the watchdog's frame, never through a receive deadline. Shared by the control
/// and cursor queues (gpud is single-threaded: one wait at a time) and disarmed on every wait
/// exit, so a stale fire can never pass for the next wait's bound.
pub(crate) struct GpuWatchdog {
    timer: RefCell<NotifyTimer>,
    /// `[irq_ep, watchdog]` once an IRQ line is bound (`attach_irq`); `None` = the poll
    /// fallback (one yield per turn, the watchdog still bounds it) — also the state during
    /// device bring-up, before the IRQ is bound.
    waitset: RefCell<Option<Waitset>>,
}

/// What ended one bounded wait turn.
pub(crate) enum IrqWake {
    /// The GPU raised its ring-buffer IRQ.
    Irq,
    /// The watchdog fired: the bound is spent.
    Watchdog,
    /// No IRQ line — one yield elapsed; the caller re-checks the ring.
    Polled,
}

impl GpuWatchdog {
    /// Binds the watchdog timer on gpud's declared pair (pinned before gpud runs).
    pub(crate) fn bind() -> Option<Self> {
        let timer = NotifyTimer::bind(nexus_service_topology::slots::gpud::WATCHDOG).ok()?;
        Some(Self { timer: RefCell::new(timer), waitset: RefCell::new(None) })
    }

    /// The IRQ line is bound: from now on a wait is one waitset wait over `[irq_ep, watchdog]`.
    pub(crate) fn attach_irq(&self, irq_ep: u32) {
        if self.waitset.borrow().is_some() {
            return; // both queues share one watchdog; the line is attached once
        }
        let recv = self.timer.borrow().recv_slot();
        *self.waitset.borrow_mut() = Waitset::over(&[irq_ep, recv]).ok();
    }

    /// Arms the bound at `deadline_ns` (a no-op while that deadline already stands) and waits
    /// for the IRQ or the watchdog; without an IRQ line, one yield.
    fn wait_until(&self, deadline_ns: u64) -> IrqWake {
        self.timer.borrow_mut().arm_at(deadline_ns);
        match self.waitset.borrow().as_ref() {
            Some(ws) => match ws.wait() {
                Ok(0) => IrqWake::Irq,
                // The watchdog member woke us: a fire, or the kernel's EOF latch (spurious —
                // read as one polled turn, the caller re-checks the ring).
                Ok(_) => {
                    if self.timer.borrow_mut().drain() {
                        IrqWake::Watchdog
                    } else {
                        IrqWake::Polled
                    }
                }
                Err(_) => IrqWake::Polled,
            },
            None => {
                let _ = nexus_abi::yield_();
                if self.timer.borrow_mut().drain() {
                    IrqWake::Watchdog
                } else {
                    IrqWake::Polled
                }
            }
        }
    }

    fn disarm(&self) {
        self.timer.borrow_mut().disarm();
    }
}

impl CtrlQueue {
    /// Bind this queue to a GPU ring-buffer IRQ so the completion wait can BLOCK
    /// on the interrupt instead of busy-polling. `irq_ep` is the endpoint cap slot
    /// the kernel routes the PLIC source to (set via `irq_bind`); `irq_num` is that
    /// source. Both 0 keeps the legacy spin+yield path.
    pub(crate) fn set_gpu_irq(&mut self, irq_num: u32, irq_ep: u32) {
        self.irq_num = irq_num;
        self.irq_ep = irq_ep;
        if let Some(wd) = self.watchdog.as_ref() {
            wd.attach_irq(irq_ep);
        }
    }

    /// The queue's wait bound (TASK-0054C P2-b), shared with the other queue; set at
    /// creation so device bring-up commands are bounded too.
    pub(crate) fn set_watchdog(&mut self, watchdog: Option<Rc<GpuWatchdog>>) {
        self.watchdog = watchdog;
    }

    /// so the ring can never deadlock.
    pub(super) fn alloc_free_slot(&mut self) -> Result<RingSlot, GfxError> {
        if let Some(slot) = self.find_free_slot() {
            return Ok(slot);
        }
        // No watchdog (its bind failed at start, reported there): a full ring has no bound —
        // refuse rather than hang or spin.
        let Some(wd) = self.watchdog.clone() else {
            return Err(GfxError::MmioFault);
        };
        // The clock is read once to COMPUTE the bound; the kernel timer owns it from here.
        let deadline = nexus_abi::nsec()
            .map_err(|_| GfxError::MmioFault)?
            .saturating_add(GPU_WAIT_DEADLINE_NS);
        loop {
            // Park-safe re-arm: a completion IRQ that fired while nobody was
            // waiting (the pipelined enqueue phase) left the source claim-MASKED
            // at the PLIC — the kernel completes only on `irq_complete`. Parking
            // with the source masked sleeps the FULL bound even though the
            // completion already landed silently in the used-ring (the observed
            // serialized-500ms boot stalls). Re-arm first, then re-harvest to
            // close the ack race (a completion that lands after the re-arm
            // asserts the now-armed line and is delivered as a queued message).
            if self.irq_ep != 0 {
                self.ack_gpu_irq();
                if let Some(slot) = self.find_free_slot() {
                    wd.disarm();
                    return Ok(slot);
                }
            }
            match wd.wait_until(deadline) {
                IrqWake::Irq => self.note_irq_wake(),
                IrqWake::Polled => {}
                IrqWake::Watchdog => {
                    IRQ_DEADLINE_EXPIRED_COUNT.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
                    // Degraded recovery: abandon the stuck in-flight set + resync the
                    // harvest cursor so we never wedge. Best-effort (a lost IRQ only).
                    self.ring.reset();
                    self.last_used = unsafe { core::ptr::read_volatile(&(*self.used).idx) };
                    if self.irq_ep != 0 {
                        self.ack_gpu_irq();
                    }
                    // `reset` emptied the ring, so this reservation always succeeds.
                    return self
                        .ring
                        .try_alloc()
                        .map(|(slot, _)| RingSlot(slot.0 as u16))
                        .ok_or(GfxError::MmioFault);
                }
            }
            if let Some(slot) = self.find_free_slot() {
                if self.irq_ep != 0 {
                    self.ack_gpu_irq();
                }
                wd.disarm();
                return Ok(slot);
            }
        }
    }

    /// (`debug_println`, not `trace_line`, so a quiet boot shows it).
    fn note_irq_wake(&self) {
        IRQ_WAKE_COUNT.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        if !GPU_IRQ_WAKE_LOGGED.swap(true, core::sync::atomic::Ordering::Relaxed) {
            let _ = nexus_abi::debug_println("gpud: gpu irq wake");
        }
    }

    /// blocks the present).
    pub(super) fn wait_slot(&mut self, slot: RingSlot) -> Result<(), GfxError> {
        let dk_slot = nexus_driverkit::Slot(slot.0 as u8);
        self.harvest();
        if !self.ring.is_in_flight(dk_slot) {
            if self.irq_ep != 0 {
                self.ack_gpu_irq();
            }
            return Ok(());
        }
        let Some(wd) = self.watchdog.clone() else {
            return Err(GfxError::MmioFault);
        };
        let deadline = nexus_abi::nsec()
            .map_err(|_| GfxError::MmioFault)?
            .saturating_add(GPU_WAIT_DEADLINE_NS);
        loop {
            // Park-safe re-arm (see `alloc_free_slot`): complete any stale
            // claim-masked IRQ so the wake for OUR completion can be delivered,
            // then re-harvest to close the ack race before parking.
            if self.irq_ep != 0 {
                self.ack_gpu_irq();
                self.harvest();
                if !self.ring.is_in_flight(dk_slot) {
                    wd.disarm();
                    return Ok(());
                }
            }
            match wd.wait_until(deadline) {
                IrqWake::Irq => self.note_irq_wake(),
                IrqWake::Polled => {}
                IrqWake::Watchdog => {
                    IRQ_DEADLINE_EXPIRED_COUNT.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
                    // Abandon the stuck slot (degraded, lost-IRQ only): free it WITHOUT counting
                    // a completion (the command never finished), so a fence can't jump past it.
                    self.ring.abandon(dk_slot);
                    if self.irq_ep != 0 {
                        self.ack_gpu_irq();
                    }
                    return Err(GfxError::MmioFault);
                }
            }
            self.harvest();
            if !self.ring.is_in_flight(dk_slot) {
                if self.irq_ep != 0 {
                    self.ack_gpu_irq();
                }
                wd.disarm();
                return Ok(());
            }
        }
    }
}
