// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The console's transmit queue on the target (TASK-0330 S2a): a console byte goes to
//! the queue (`crate::console_tx`, host-tested) and the UART's own transmitter-empty interrupt
//! sends it in bursts — a console write holds the kernel lock for a copy, never for the serial
//! time (10.5 ms a line on the board, `docs/board/measurements/2026-10-10-mgs-baseline/`). A
//! pusher that finds the transmitter idle fills it at once (no wait: the FIFO is empty), so the
//! queue never depends on the line's trigger mode; the interrupt sends the rest and is live only
//! while the queue holds bytes. A full queue (a bring-up burst) sends a burst of its oldest
//! bytes now, polled, so the order the ring recorded holds (RFC-0107's trace contract); a panic
//! turns the queue off and drains it before its last words. Without a line in the tree the
//! timer tick drains it. `platform` owns the registers.
//!
//! The board's UART (`docs/board/measurements/2026-10-12-console-off-lock/`): a PXA-style
//! 16550 whose IER carries the unit enable (bit 6) and a DMA-requests bit (bit 7) — with bit 7
//! set the empty transmitter raises a DMA request, not the interrupt (the vendor console's
//! mode, IER=0xC0 on the stock system; its IRQ 42 counts zero there) — and whose MCR OUT2
//! (bit 3) gates the interrupt output (the boot loader leaves it clear).
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `KSELFTEST: console off lock ok (` on every lane and the board; the queue's
//!   order and bound on the host (`crate::console_tx`); the trace contract (ring = UART order)
//! INVARIANTS: a byte enters the queue only after the ring recorded it; the queue's order is the
//!   UART's; the transmitter is written only under the queue's lock; the interrupt's enable and
//!   clear are decided under the queue's lock.

use core::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};

use super::plic::IrqId;
use crate::console_tx::TxRing;
use crate::sync::spin_irq::SpinIrqLock;

/// Console bytes go to the queue — or, before the runtime and in a panic, straight to the
/// transmitter.
static CONSOLE_ASYNC: AtomicBool = AtomicBool::new(false);
/// The console's interrupt is enabled at the PLIC and drains the queue.
static CONSOLE_IRQ_LIVE: AtomicBool = AtomicBool::new(false);
/// The transmit queue; IRQ-safe, since the drain runs in the external-interrupt trap and a
/// push may run under the kernel lock on another hart.
static TX: SpinIrqLock<TxRing> = SpinIrqLock::new(TxRing::new());

const UART_IER: usize = 0x1;
const UART_IIR: usize = 0x2;
const UART_FCR: usize = 0x2;
const UART_MCR: usize = 0x4;
/// IER: the transmitter-empty interrupt enable.
const IER_THRE: u8 = 0x02;
/// IER bits kept as the firmware left them: bit 6 (a PXA-style UART's unit enable; reserved
/// and ignored on a plain 16550) and bit 5 (its line coding). Cleared with the kernel's own
/// four enables: bit 7 (DMA requests in place of the transmitter's interrupt) and bit 4 (a
/// receive timeout nobody reads).
const IER_KEEP: u8 = 0x60;
/// MCR OUT2: on the 16550 family the gate between the UART's interrupt and the controller.
const MCR_OUT2: u8 = 0x08;
/// FCR: the FIFOs on — a burst per empty-transmitter interrupt.
const FCR_FIFO_ENABLE: u8 = 0x01;
/// Bytes written per empty transmitter: the 16550's FIFO, the smallest in the family.
const FIFO_BURST: usize = 16;
/// How long a pusher (or the drain) waits for the queue's lock before it gives up: a pusher
/// writes the byte straight (a hart stuck under the lock must not stop another's console); the
/// drain leaves the bytes to the pusher, who fills the idle transmitter itself.
const TX_LOCK_TRIES: usize = 1 << 16;

/// The queue's witness for the boot-end record: bursts the interrupt sent, bursts a pusher sent
/// into an idle transmitter, pushes that found the queue full (a burst went out polled), pushes
/// that found the lock held past the bound (the byte went straight), and the deepest the queue
/// got.
static TX_IRQ_BURSTS: AtomicU32 = AtomicU32::new(0);
static TX_KICKS: AtomicU32 = AtomicU32::new(0);
static TX_FULL: AtomicU32 = AtomicU32::new(0);
static TX_LOCK_FALLBACK: AtomicU32 = AtomicU32::new(0);
static TX_MAX_DEPTH: AtomicUsize = AtomicUsize::new(0);

/// `(irq_bursts, kicks, full, lock_fallback, max_depth)` since boot.
pub fn console_tx_report() -> (u32, u32, u32, u32, usize) {
    (
        TX_IRQ_BURSTS.load(Ordering::Relaxed),
        TX_KICKS.load(Ordering::Relaxed),
        TX_FULL.load(Ordering::Relaxed),
        TX_LOCK_FALLBACK.load(Ordering::Relaxed),
        TX_MAX_DEPTH.load(Ordering::Relaxed),
    )
}

fn transmitter_idle(base: usize) -> bool {
    super::platform::uart_read_reg(base, super::platform::UART_LSR) & super::platform::LSR_TX_IDLE
        != 0
}

/// Up to one FIFO burst from the queue into the transmitter — the caller saw it idle.
fn write_burst(base: usize, q: &mut TxRing) {
    for _ in 0..FIFO_BURST {
        match q.pop() {
            Some(b) => super::platform::uart_write_reg(base, super::platform::UART_TX, b),
            None => break,
        }
    }
}

/// Waits (bounded) for the transmitter's FIFO to empty, then writes up to one FIFO burst from
/// the queue — never a wait per byte: with the FIFO on, "empty" means the whole FIFO, and a
/// byte-wise wait cost the FIFO's drain time per byte (9.6 ms a console syscall on the board,
/// 2026-10-12, the first cut of this queue).
fn uart_burst_polled(base: usize, q: &mut TxRing) {
    let bound = super::platform::ns_to_ticks(2_000_000);
    let t0 = crate::arch::riscv::read_time();
    while !transmitter_idle(base) {
        if bound != 0 && crate::arch::riscv::read_time().wrapping_sub(t0) > bound {
            return;
        }
    }
    write_burst(base, q);
}

/// The queue's lock, waited for up to the bound; `None` when a hart holds it past that.
fn lock_bounded() -> Option<crate::sync::spin_irq::SpinIrqGuard<'static, TxRing>> {
    for _ in 0..TX_LOCK_TRIES {
        if let Some(q) = TX.try_lock() {
            return Some(q);
        }
        core::hint::spin_loop();
    }
    None
}

/// Whether console bytes go to the queue now.
pub fn enabled() -> bool {
    CONSOLE_ASYNC.load(Ordering::Acquire)
}

/// One byte into the transmit queue (TASK-0330 S2a). An idle transmitter is filled at once (no
/// wait); the console's interrupt sends the rest. A full queue (a bring-up burst) sends a burst
/// of its oldest bytes now, polled, so the order holds and the hold stays bounded; a lock held
/// past the bound sends the byte straight.
pub fn enqueue(base: usize, byte: u8) {
    let Some(mut q) = lock_bounded() else {
        TX_LOCK_FALLBACK.fetch_add(1, Ordering::Relaxed);
        super::platform::uart_put_polled(base, byte);
        return;
    };
    if !q.push(byte) {
        TX_FULL.fetch_add(1, Ordering::Relaxed);
        uart_burst_polled(base, &mut q);
        let _ = q.push(byte);
    }
    TX_MAX_DEPTH.fetch_max(q.len(), Ordering::Relaxed);
    if CONSOLE_IRQ_LIVE.load(Ordering::Relaxed) {
        // The transmitter-empty interrupt is live while the queue holds bytes (decided under
        // the queue's lock, as its clearing is — no byte waits with it off).
        let ier = super::platform::uart_read_reg(base, UART_IER);
        if ier & IER_THRE == 0 {
            super::platform::uart_write_reg(base, UART_IER, (ier & IER_KEEP) | IER_THRE);
        }
    }
    if transmitter_idle(base) {
        // Idle now: fill it ourselves — the interrupt's edge may have passed while the lock
        // was held, and the first bytes of a line need not wait for one.
        TX_KICKS.fetch_add(1, Ordering::Relaxed);
        write_burst(base, &mut q);
    }
}

/// A claimed source is the console's: its queue drained and the source completed — `true`;
/// otherwise `false` and the caller's business.
pub fn console_irq_fired(irq: IrqId) -> bool {
    if super::platform::console_irq() != Some(irq) {
        return false;
    }
    console_tx_drain();
    super::plic::complete_current(irq);
    true
}

/// A burst of the queue into the empty transmitter; the interrupt is turned off with the last
/// byte. A hart holding the lock past the bound: the bytes are its — it fills the idle
/// transmitter before it releases.
fn console_tx_drain() {
    let base = super::platform::uart_base();
    if base == 0 {
        return;
    }
    let Some(mut q) = lock_bounded() else {
        return;
    };
    if transmitter_idle(base) {
        TX_IRQ_BURSTS.fetch_add(1, Ordering::Relaxed);
        write_burst(base, &mut q);
    }
    if q.is_empty() && CONSOLE_IRQ_LIVE.load(Ordering::Relaxed) {
        let ier = super::platform::uart_read_reg(base, UART_IER);
        if ier & IER_THRE != 0 {
            super::platform::uart_write_reg(base, UART_IER, ier & IER_KEEP);
        }
    }
}

/// Whether the console's line is enabled in the boot hart's PLIC context (a readback for the
/// boot line; the host has no PLIC).
fn plic_enabled(irq: IrqId) -> bool {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        super::plic::selftest_source_enabled(irq, crate::types::CpuId::BOOT)
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        let _ = irq;
        false
    }
}

/// From the runtime on, console bytes go to the queue (TASK-0330 S2a): the FIFOs on, the
/// receive, line-status and DMA requests off, the interrupt output gated open (MCR OUT2), the
/// console's line enabled at the PLIC for the boot hart. The boot line carries the registers
/// read back, so a board cycle decides the UART side from its trace. Without a line in the
/// tree the queue drains at the timer tick instead.
pub fn console_async_enable() {
    let base = super::platform::uart_base();
    if base == 0 {
        return;
    }
    let irq = super::platform::console_irq();
    let mut readback = (0u8, 0u8, 0u8, false);
    if let Some(irq) = irq {
        super::platform::uart_write_reg(base, UART_FCR, FCR_FIFO_ENABLE);
        let ier = super::platform::uart_read_reg(base, UART_IER);
        super::platform::uart_write_reg(base, UART_IER, ier & IER_KEEP);
        let mcr = super::platform::uart_read_reg(base, UART_MCR);
        super::platform::uart_write_reg(base, UART_MCR, mcr | MCR_OUT2);
        super::plic::enable_source(irq, crate::types::CpuId::BOOT);
        readback = (
            super::platform::uart_read_reg(base, UART_IER),
            super::platform::uart_read_reg(base, UART_MCR),
            super::platform::uart_read_reg(base, UART_IIR),
            plic_enabled(irq),
        );
        CONSOLE_IRQ_LIVE.store(true, Ordering::Release);
    }
    CONSOLE_ASYNC.store(true, Ordering::Release);
    log_info!(
        target: "boot",
        "KINIT: console tx queue (irq={} fifo_burst={} ier={:#04x} mcr={:#04x} iir={:#04x} plic_enabled={})",
        irq.map(|i| i.raw()).unwrap_or(0),
        FIFO_BURST,
        readback.0,
        readback.1,
        readback.2,
        readback.3
    );
}

/// Back to the transmitter, now: the queue's bytes polled out first (a panic's last words
/// follow them in order).
pub fn console_async_disable_and_drain() {
    CONSOLE_ASYNC.store(false, Ordering::Release);
    let base = super::platform::uart_base();
    if base == 0 {
        return;
    }
    if let Some(mut q) = TX.try_lock() {
        while !q.is_empty() {
            uart_burst_polled(base, &mut q);
        }
    }
}

/// The timer tick's drain, for a console whose tree names no interrupt line.
pub fn console_tx_tick() {
    if CONSOLE_ASYNC.load(Ordering::Relaxed) && !CONSOLE_IRQ_LIVE.load(Ordering::Relaxed) {
        console_tx_drain();
    }
}
