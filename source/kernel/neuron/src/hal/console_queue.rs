// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The console's transmit queue on the target (TASK-0330 S2a, S2b): the queue is the
//! console ring itself (RFC-0107) — the bytes between the serial line's cursor
//! (`crate::console_tx`, host-tested) and the ring's head. A console write records its byte and
//! kicks the line: an idle transmitter is filled at once (no wait — the FIFO is empty), and the
//! UART's own transmitter-empty interrupt sends the rest in FIFO bursts, live only while the line
//! owes bytes. No writer ever waits for the line: S2a's 8 KiB queue sent its oldest bytes polled
//! when full, and every board bring-up filled it — the console syscalls held the kernel lock for
//! the serial time of their lines again (20 ms), a crash report held its hart for about 134 ms
//! (S2b's measurement). The ring holds a whole boot's console, so the line falls behind and
//! catches up without losing a byte; a line the writers lap (more than the ring behind) skips to
//! the oldest byte kept and says so in one line. A panic stops the reading and sends what the
//! line owes before its last words. Without an interrupt line in the tree the timer tick drains
//! the line, and the tick is the backstop for a kick that found the line's lock held.
//! `platform` owns the registers.
//!
//! The board's UART (`docs/board/measurements/2026-10-12-console-off-lock/`): a PXA-style
//! 16550 whose IER carries the unit enable (bit 6) and a DMA-requests bit (bit 7) — with bit 7
//! set the empty transmitter raises a DMA request, not the interrupt (the vendor console's
//! mode, IER=0xC0 on the stock system; its IRQ 42 counts zero there) — and whose MCR OUT2
//! (bit 3) gates the interrupt output (the boot loader leaves it clear).
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `KSELFTEST: console off lock ok (` on every lane and the board; the cursor's
//!   order and lap rule on the host (`crate::console_tx`); the trace contract (every QEMU lane
//!   compares the ring with the UART log byte for byte)
//! INVARIANTS: the line sends the ring's bytes in the ring's order; no console write waits for
//!   the transmitter; the transmitter and the cursor are touched only under the line's lock; the
//!   interrupt's enable and clear are decided under it.

use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

use super::console_ring;
use super::platform::{uart_read_reg, uart_write_reg, LSR_TX_IDLE, UART_LSR, UART_TX};
use super::plic::IrqId;
use crate::console_tx::{lap_note, Cursor, FIFO_BURST, NOTE_MAX};
use crate::sync::spin_irq::SpinIrqLock;

/// The serial line reads the ring — or, before the runtime and in a panic, each byte goes to
/// the transmitter as it is written.
static CONSOLE_ASYNC: AtomicBool = AtomicBool::new(false);
/// The console's interrupt is enabled at the PLIC and drains the line.
static CONSOLE_IRQ_LIVE: AtomicBool = AtomicBool::new(false);

/// The serial line: its cursor in the ring and the note it owes after a lap.
struct Line {
    cursor: Cursor,
    note: [u8; NOTE_MAX],
    note_len: usize,
    note_at: usize,
}

impl Line {
    const fn new() -> Self {
        Self { cursor: Cursor::new(), note: [0; NOTE_MAX], note_len: 0, note_at: 0 }
    }

    fn note_pending(&self) -> bool {
        self.note_at < self.note_len
    }

    /// Whether the line still owes bytes up to `head`.
    fn owes(&self, head: u64) -> bool {
        self.note_pending() || self.cursor.behind(head) > 0
    }

    /// The line skipped `n` bytes: counted, and said in one line before its next byte.
    fn owe_note(&mut self, n: u64) {
        TX_LAPPED.fetch_add(n, Ordering::Relaxed);
        self.note_len = lap_note(n, &mut self.note);
        self.note_at = 0;
    }
}

/// The line; IRQ-safe, since the drain runs in the external-interrupt trap and a kick may run
/// under the kernel lock on another hart.
static LINE: SpinIrqLock<Line> = SpinIrqLock::new(Line::new());

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
/// How long a kick (or the drain) waits for the line's lock before it gives up: a holder sends
/// one burst within microseconds; a kick that gives up leaves its byte in the ring for the
/// holder, the interrupt or the tick.
const TX_LOCK_TRIES: usize = 1 << 16;
/// How long a panic's drain waits for the transmitter to empty: twenty byte times at the
/// slowest console rate — a transmitter that stops, whatever stops it, ends the drain.
const PANIC_IDLE_WAIT_NS: u64 = 2_000_000;

/// The line's witness for the boot-end record: bursts the line's drain sent (its interrupt, or
/// the tick), bursts a writer sent into an idle transmitter, the farthest the line fell behind
/// the writers (bytes), the bytes it skipped because the writers lapped it, and kicks that found
/// the line's lock held past the bound.
static TX_IRQ_BURSTS: AtomicU32 = AtomicU32::new(0);
static TX_KICKS: AtomicU32 = AtomicU32::new(0);
static TX_BEHIND_MAX: AtomicU64 = AtomicU64::new(0);
static TX_LAPPED: AtomicU64 = AtomicU64::new(0);
static TX_LOCK_FALLBACK: AtomicU32 = AtomicU32::new(0);

/// `(irq_bursts, kicks, behind_max, lapped, lock_fallback)` since boot.
pub fn console_tx_report() -> (u32, u32, u64, u64, u32) {
    (
        TX_IRQ_BURSTS.load(Ordering::Relaxed),
        TX_KICKS.load(Ordering::Relaxed),
        TX_BEHIND_MAX.load(Ordering::Relaxed),
        TX_LAPPED.load(Ordering::Relaxed),
        TX_LOCK_FALLBACK.load(Ordering::Relaxed),
    )
}

fn transmitter_idle(base: usize) -> bool {
    uart_read_reg(base, UART_LSR) & LSR_TX_IDLE != 0
}

/// Up to one FIFO burst into the transmitter, which the caller saw idle: the note the line owes
/// first, then the ring's bytes up to `head`. A burst the writers reached while it was read goes
/// out as a note, not as bytes.
fn write_burst(base: usize, line: &mut Line, head: u64) {
    let mut room = FIFO_BURST;
    loop {
        while room > 0 && line.note_pending() {
            uart_write_reg(base, UART_TX, line.note[line.note_at]);
            line.note_at += 1;
            room -= 1;
        }
        if room == 0 {
            return;
        }
        let burst = line.cursor.plan(head, console_ring::CAPACITY, room);
        if burst.lapped > 0 {
            line.owe_note(burst.lapped);
            continue;
        }
        if burst.len == 0 {
            return;
        }
        let mut bytes = [0u8; FIFO_BURST];
        for (i, b) in bytes.iter_mut().take(burst.len).enumerate() {
            *b = console_ring::byte_at(burst.from + i as u64);
        }
        // The bytes are read before the head is read again (a reader's half of a sequence
        // check): a writer that reached them shows in that head.
        core::sync::atomic::fence(Ordering::Acquire);
        if !line.cursor.commit(burst, console_ring::written(), console_ring::CAPACITY) {
            line.owe_note(burst.len as u64);
            continue;
        }
        for &b in &bytes[..burst.len] {
            uart_write_reg(base, UART_TX, b);
        }
        return;
    }
}

/// The transmitter-empty interrupt on — while the line owes bytes, decided under its lock.
fn thre_on(base: usize) {
    if CONSOLE_IRQ_LIVE.load(Ordering::Relaxed) {
        let ier = uart_read_reg(base, UART_IER);
        if ier & IER_THRE == 0 {
            uart_write_reg(base, UART_IER, (ier & IER_KEEP) | IER_THRE);
        }
    }
}

/// The transmitter-empty interrupt off — the line owes nothing.
fn thre_off(base: usize) {
    if CONSOLE_IRQ_LIVE.load(Ordering::Relaxed) {
        let ier = uart_read_reg(base, UART_IER);
        if ier & IER_THRE != 0 {
            uart_write_reg(base, UART_IER, ier & IER_KEEP);
        }
    }
}

/// The line's lock, waited for up to the bound; `None` when a hart holds it past that.
fn lock_bounded() -> Option<crate::sync::spin_irq::SpinIrqGuard<'static, Line>> {
    for _ in 0..TX_LOCK_TRIES {
        if let Some(line) = LINE.try_lock() {
            return Some(line);
        }
        core::hint::spin_loop();
    }
    None
}

/// Whether the serial line reads the ring now.
pub fn enabled() -> bool {
    CONSOLE_ASYNC.load(Ordering::Acquire)
}

/// A writer's byte is in the ring (TASK-0330 S2b): an idle transmitter is filled now, otherwise
/// the interrupt is made live while the line owes bytes. Never a wait for the transmitter; a
/// lock held past the bound leaves the byte in the ring for the holder, the interrupt or the
/// tick.
pub fn kick() {
    let base = super::platform::uart_base();
    if base == 0 {
        return;
    }
    let Some(mut line) = lock_bounded() else {
        TX_LOCK_FALLBACK.fetch_add(1, Ordering::Relaxed);
        return;
    };
    let head = console_ring::written();
    TX_BEHIND_MAX.fetch_max(line.cursor.behind(head), Ordering::Relaxed);
    if transmitter_idle(base) {
        TX_KICKS.fetch_add(1, Ordering::Relaxed);
        write_burst(base, &mut line, head);
    }
    if line.owes(head) {
        thre_on(base);
    }
}

/// A claimed source is the console's: the line drained and the source completed — `true`;
/// otherwise `false` and the caller's business.
pub fn console_irq_fired(irq: IrqId) -> bool {
    if super::platform::console_irq() != Some(irq) {
        return false;
    }
    console_tx_drain();
    super::plic::complete_current(irq);
    true
}

/// A burst into the empty transmitter; the interrupt goes off once the line owes nothing. A hart
/// holding the lock past the bound: the bytes are its — it fills the idle transmitter or turns
/// the interrupt on before it releases.
fn console_tx_drain() {
    let base = super::platform::uart_base();
    if base == 0 {
        return;
    }
    let Some(mut line) = lock_bounded() else {
        return;
    };
    let head = console_ring::written();
    if transmitter_idle(base) {
        TX_IRQ_BURSTS.fetch_add(1, Ordering::Relaxed);
        write_burst(base, &mut line, head);
    }
    if line.owes(head) {
        thre_on(base);
    } else {
        thre_off(base);
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

/// From the runtime on, the serial line reads the ring (TASK-0330 S2a, S2b): the FIFOs on, the
/// receive, line-status and DMA requests off, the interrupt output gated open (MCR OUT2), the
/// console's line enabled at the PLIC for the boot hart. The boot line carries the registers
/// read back, so a board cycle decides the UART side from its trace. Without a line in the tree
/// the line drains at the timer tick instead.
pub fn console_async_enable() {
    let base = super::platform::uart_base();
    if base == 0 {
        return;
    }
    let irq = super::platform::console_irq();
    let mut readback = (0u8, 0u8, 0u8, false);
    if let Some(irq) = irq {
        uart_write_reg(base, UART_FCR, FCR_FIFO_ENABLE);
        let ier = uart_read_reg(base, UART_IER);
        uart_write_reg(base, UART_IER, ier & IER_KEEP);
        let mcr = uart_read_reg(base, UART_MCR);
        uart_write_reg(base, UART_MCR, mcr | MCR_OUT2);
        super::plic::enable_source(irq, crate::types::CpuId::BOOT);
        readback = (
            uart_read_reg(base, UART_IER),
            uart_read_reg(base, UART_MCR),
            uart_read_reg(base, UART_IIR),
            plic_enabled(irq),
        );
        CONSOLE_IRQ_LIVE.store(true, Ordering::Release);
    }
    // The switch between two bytes: every byte before it went to the transmitter as it was
    // written, every byte after it is the line's to read.
    console_ring::with_lock(|| {
        LINE.lock().cursor.follow(console_ring::written());
        CONSOLE_ASYNC.store(true, Ordering::Release);
    });
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

/// Back to the transmitter, now (a panic): what the line owes up to the head as it stands goes
/// out first, polled — the dying kernel's last words follow it in order. A transmitter that
/// stops ends the wait.
pub fn console_async_disable_and_drain() {
    CONSOLE_ASYNC.store(false, Ordering::Release);
    let base = super::platform::uart_base();
    if base == 0 {
        return;
    }
    let Some(mut line) = lock_bounded() else {
        return;
    };
    let end = console_ring::written();
    while line.owes(end) {
        if !wait_idle(base) {
            return;
        }
        write_burst(base, &mut line, end);
    }
}

/// Waits, bounded, for the transmitter to empty.
fn wait_idle(base: usize) -> bool {
    let bound = super::platform::ns_to_ticks(PANIC_IDLE_WAIT_NS);
    let t0 = crate::arch::riscv::read_time();
    while !transmitter_idle(base) {
        if bound != 0 && crate::arch::riscv::read_time().wrapping_sub(t0) > bound {
            return false;
        }
    }
    true
}

/// The timer tick's drain: the line of a console whose tree names no interrupt line, and the
/// backstop for a byte whose kick found the line's lock held.
pub fn console_tx_tick() {
    if !CONSOLE_ASYNC.load(Ordering::Relaxed) {
        return;
    }
    let base = super::platform::uart_base();
    if base == 0 {
        return;
    }
    let Some(mut line) = LINE.try_lock() else {
        return;
    };
    let head = console_ring::written();
    if !line.owes(head) {
        return;
    }
    if transmitter_idle(base) {
        TX_IRQ_BURSTS.fetch_add(1, Ordering::Relaxed);
        write_burst(base, &mut line, head);
    }
    if line.owes(head) {
        thre_on(base);
    }
}
