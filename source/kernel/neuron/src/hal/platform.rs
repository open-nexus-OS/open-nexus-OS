// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the kernel's platform — console, interrupt controller, timer,
//! timebase, hart count — built ONCE from the device tree in `a1` and from
//! nothing else (RFC-0098 C3, TASK-0245). This file replaces `hal/virt.rs`: the
//! same HAL traits, but every number a machine differs in is a value read at
//! boot, held in lock-free statics so the earliest console byte and the trap
//! path can use it without a lock or a heap.
//!
//! WHY STATICS: the console is needed before the heap exists and inside trap
//! handlers; the PLIC context is read on every claim; the timebase converts every
//! `nsec`. Relaxed atomics written once in [`init_from_fdt`] (paging off, boot
//! hart, before any other hart runs) and read everywhere is the cheapest correct
//! shape. Before `init_from_fdt` the console base is 0 and writes are dropped —
//! a boot without a tree has no console, and the harness sees silence, never a
//! wild write to a guessed address.
//!
//! WHAT IS READ: `/chosen/stdout-path` → the UART node (`reg`, `reg-shift`,
//! `reg-io-width`; both a byte-stride `ns16550a` and the board's 4-byte-stride
//! 16550-class part are one driver); the PLIC (`reg`, `riscv,ndev`, the S-mode
//! context of every hart from `interrupts-extended` — READ, no longer
//! `2·hart+1`); `/cpus` (`timebase-frequency`, hart count, `sstc` in the boot
//! hart's ISA extensions → `stimecmp` instead of an SBI call per tick).
//!
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: QEMU marker ladder (`KSELFTEST: platform from fdt ok (…)` prints
//!   every value read; the smp/smp1 lanes exercise contexts and timers); the
//!   parser is host-tested in nexus-fdt
//! RFC: docs/rfcs/RFC-0098-board-support-contract-fdt-truth-boot-chain.md

use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};

use super::{IrqCtl, Timer, Tlb, Uart};
use crate::arch::riscv;

/// Console registers: base, register shift (`reg-shift`), access width in bytes.
static UART_BASE: AtomicUsize = AtomicUsize::new(0);
static UART_LEN: AtomicUsize = AtomicUsize::new(0);
static UART_SHIFT: AtomicUsize = AtomicUsize::new(0);
static UART_WIDTH: AtomicUsize = AtomicUsize::new(1);
/// Interrupt controller.
static PLIC_BASE: AtomicUsize = AtomicUsize::new(0);
static PLIC_LEN: AtomicUsize = AtomicUsize::new(0);
static PLIC_NDEV: AtomicU32 = AtomicU32::new(0);
/// S-mode PLIC context per cpu index; `NO_CONTEXT` when the tree lists none.
const NO_CONTEXT: u32 = u32::MAX;
static PLIC_SCTX: [AtomicU32; crate::smp::MAX_CPUS] =
    [const { AtomicU32::new(NO_CONTEXT) }; crate::smp::MAX_CPUS];
/// Timebase and timer source. `NS_MUL`/`NS_DIV` are `1e9/hz` reduced by their
/// gcd at init (10 MHz → 100/1, 24 MHz → 125/3), so every conversion is one
/// 64-bit multiply and one 64-bit divide on the syscall path — exact, and no
/// 128-bit software division per `nsec`.
static TIMEBASE_HZ: AtomicU64 = AtomicU64::new(0);
static NS_MUL: AtomicU64 = AtomicU64::new(0);
static NS_DIV: AtomicU64 = AtomicU64::new(1);
static TIMER_SSTC: AtomicBool = AtomicBool::new(false);
/// The harts' Zicbom cache-block size from the tree (`riscv,cbom-block-size`),
/// 0 when the ISA does not list `zicbom` (RFC-0098 C4).
static CBOM_BLOCK: AtomicUsize = AtomicUsize::new(0);
static HART_COUNT: AtomicUsize = AtomicUsize::new(0);
/// Bit `h` set: the tree lists hart `h` as a cpu the OS may use (`status` okay or absent).
static HART_MASK: AtomicUsize = AtomicUsize::new(0);
/// `/memory` banks `(base, len)`, in tree order; the direct map and the frame
/// allocator (TASK-0286) are built over these.
static MEM_BANKS: [(AtomicUsize, AtomicUsize); MAX_BANKS] =
    [const { (AtomicUsize::new(0), AtomicUsize::new(0)) }; MAX_BANKS];
static MEM_BANK_COUNT: AtomicUsize = AtomicUsize::new(0);
/// Banks the platform records (the board has two).
pub const MAX_BANKS: usize = 4;

/// UART register numbers (16550 register map; the stride comes from the tree).
const UART_TX: usize = 0x0;
const UART_LSR: usize = 0x5;
const LSR_TX_IDLE: u8 = 1 << 5;
/// `stimecmp` (Sstc), by number: the pinned assembler predates the name.
const CSR_STIMECMP: u16 = 0x14d;

/// What went wrong when the tree could not describe a platform.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlatformError {
    NoTree,
    Parse(nexus_fdt::Error),
    NoConsole,
    NoPlic,
    NoCpus,
}

/// Build the platform from the tree. Paging OFF (the tree is read at its physical
/// address), boot hart only, exactly once, after BSS is zeroed. Returns the error
/// instead of panicking: the caller decides, and without a console a panic could
/// not be printed anyway.
pub fn init_from_fdt(bytes: Option<&[u8]>) -> Result<(), PlatformError> {
    let bytes = bytes.ok_or(PlatformError::NoTree)?;
    let fdt = nexus_fdt::Fdt::new(bytes).map_err(PlatformError::Parse)?;

    // Console.
    let uart = fdt.stdout().ok_or(PlatformError::NoConsole)?;
    let reg = uart.reg(0).map_err(PlatformError::Parse)?.ok_or(PlatformError::NoConsole)?;
    let shift = uart.prop_u32("reg-shift").map(|v| v as usize).unwrap_or(0);
    let width = uart.prop_u32("reg-io-width").map(|v| v as usize).unwrap_or(1);
    UART_SHIFT.store(shift, Ordering::Relaxed);
    UART_WIDTH.store(if width == 4 { 4 } else { 1 }, Ordering::Relaxed);
    UART_LEN.store(reg.size as usize, Ordering::Relaxed);
    // The base is stored LAST: from here on the console is live.
    UART_BASE.store(reg.addr as usize, Ordering::Release);

    // PLIC + the S-mode context of every hart we may run.
    let plic = fdt.plic().ok_or(PlatformError::NoPlic)?;
    let reg = plic.reg(0).map_err(PlatformError::Parse)?.ok_or(PlatformError::NoPlic)?;
    PLIC_BASE.store(reg.addr as usize, Ordering::Relaxed);
    PLIC_LEN.store(reg.size as usize, Ordering::Relaxed);
    PLIC_NDEV.store(plic.plic_ndev().unwrap_or(0), Ordering::Relaxed);
    for (hart, ctx) in plic.plic_s_contexts() {
        if let Some(slot) = PLIC_SCTX.get(hart as usize) {
            slot.store(ctx, Ordering::Relaxed);
        }
    }

    // Harts, timebase, timer source.
    let cpus = fdt.cpus().map_err(|_| PlatformError::NoCpus)?;
    let hz = u64::from(cpus.timebase_hz);
    if hz == 0 {
        return Err(PlatformError::NoCpus);
    }
    let g = gcd(1_000_000_000, hz);
    NS_MUL.store(1_000_000_000 / g, Ordering::Relaxed);
    NS_DIV.store(hz / g, Ordering::Relaxed);
    TIMEBASE_HZ.store(hz, Ordering::Relaxed);
    HART_COUNT.store(cpus.count(), Ordering::Relaxed);
    let mut mask = 0usize;
    for hart in cpus.harts().map(|c| c.hart as usize).filter(|&h| h < usize::BITS as usize) {
        mask |= 1 << hart;
    }
    HART_MASK.store(mask, Ordering::Relaxed);
    // Memory banks (RFC-0098 C4): every `/memory` bank, bounded by MAX_BANKS.
    let mut banks = 0usize;
    for bank in fdt.memory_banks().take(MAX_BANKS) {
        MEM_BANKS[banks].0.store(bank.base as usize, Ordering::Relaxed);
        MEM_BANKS[banks].1.store(bank.size as usize, Ordering::Relaxed);
        banks += 1;
    }
    MEM_BANK_COUNT.store(banks, Ordering::Relaxed);
    let sstc = cpus.harts().next().map(|c| c.has_extension("sstc")).unwrap_or(false);
    TIMER_SSTC.store(sstc, Ordering::Relaxed);
    // Zicbom: a block size the ISA cannot have (not a power of two in 16..=4096)
    // is a corrupt tree — no user cache maintenance then, never a guessed size.
    let cbom = cpus
        .harts()
        .next()
        .filter(|c| c.has_extension("zicbom"))
        .and_then(|c| c.node().prop_u32("riscv,cbom-block-size"))
        .map(|b| b as usize)
        .filter(|b| b.is_power_of_two() && (16..=4096).contains(b))
        .unwrap_or(0);
    CBOM_BLOCK.store(cbom, Ordering::Relaxed);

    Ok(())
}

/// The memory banks `(base, len)` the tree names, in tree order.
pub fn memory_banks() -> impl Iterator<Item = (usize, usize)> {
    let n = MEM_BANK_COUNT.load(Ordering::Relaxed);
    MEM_BANKS.iter().take(n).map(|(b, l)| (b.load(Ordering::Relaxed), l.load(Ordering::Relaxed)))
}

/// Console window `(base, len)`, `None` before init.
pub fn uart_window() -> Option<(usize, usize)> {
    let base = UART_BASE.load(Ordering::Acquire);
    (base != 0).then(|| (base, UART_LEN.load(Ordering::Relaxed).max(0x1000)))
}

/// PLIC window `(base, len)`, `None` before init.
pub fn plic_window() -> Option<(usize, usize)> {
    let base = PLIC_BASE.load(Ordering::Relaxed);
    (base != 0).then(|| (base, PLIC_LEN.load(Ordering::Relaxed)))
}

pub fn plic_base() -> usize {
    PLIC_BASE.load(Ordering::Relaxed)
}

/// Number of interrupt sources the controller has (`riscv,ndev`).
pub fn plic_ndev() -> u32 {
    PLIC_NDEV.load(Ordering::Relaxed)
}

/// The S-mode PLIC context of a cpu index, as the tree lists it.
pub fn plic_s_context(cpu_index: usize) -> Option<usize> {
    let ctx = PLIC_SCTX.get(cpu_index)?.load(Ordering::Relaxed);
    (ctx != NO_CONTEXT).then_some(ctx as usize)
}

pub fn timebase_hz() -> u64 {
    TIMEBASE_HZ.load(Ordering::Relaxed)
}

pub fn hart_count() -> usize {
    HART_COUNT.load(Ordering::Relaxed)
}

/// Whether the tree lists hart `hart` as one the OS may start (RFC-0098: a cpu node
/// with `status = "disabled"` is not).
pub fn hart_enabled(hart: usize) -> bool {
    hart < usize::BITS as usize && HART_MASK.load(Ordering::Relaxed) & (1 << hart) != 0
}

/// The harts' Zicbom cache-block size in bytes, 0 without Zicbom (RFC-0098 C4).
pub fn cbom_block() -> usize {
    CBOM_BLOCK.load(Ordering::Relaxed)
}

/// `senvcfg.CBIE` (bits 5:4) and `senvcfg.CBCFE` (bit 6).
const SENVCFG_CBIE_MASK: u64 = 0b11 << 4;
const SENVCFG_CBIE_FLUSH: u64 = 0b01 << 4;
const SENVCFG_CBCFE: u64 = 1 << 6;

/// Let user mode maintain its own cache lines on this hart when the tree lists
/// Zicbom (RFC-0098 C4): `cbo.clean`/`cbo.flush` allowed, and `cbo.inval`
/// executes as a flush — a driver writes back and drops lines, it can never
/// discard data. Called once per hart, next to its trap vector.
pub fn enable_user_cache_maintenance() {
    if cbom_block() != 0 {
        riscv::update_senvcfg(SENVCFG_CBIE_MASK, SENVCFG_CBIE_FLUSH | SENVCFG_CBCFE);
    }
}

/// Whether the timer is armed through `stimecmp` (Sstc) rather than SBI.
pub fn timer_uses_sstc() -> bool {
    TIMER_SSTC.load(Ordering::Relaxed)
}

/// Timer ticks per microsecond (10 on QEMU virt, 24 on the board). 0 before init.
pub fn ticks_per_us() -> u64 {
    timebase_hz() / 1_000_000
}

/// Ticks → nanoseconds: `ticks · (1e9/g) / (hz/g)`, exact for any timebase and
/// 64-bit throughout (the reduced multiplier is ≤ 1e9, ticks stay far below
/// 2^64 / 1e9 ≈ 18 000 s at 1 GHz — and 584 years at the board's 24 MHz).
pub fn ticks_to_ns(ticks: u64) -> u64 {
    let div = NS_DIV.load(Ordering::Relaxed);
    if div == 0 {
        return 0;
    }
    ticks.saturating_mul(NS_MUL.load(Ordering::Relaxed)) / div
}

/// Nanoseconds → ticks: `ns · (hz/g) / (1e9/g)`, exact, 64-bit.
pub fn ns_to_ticks(ns: u64) -> u64 {
    let mul = NS_MUL.load(Ordering::Relaxed);
    if mul == 0 {
        return 0;
    }
    ns.saturating_mul(NS_DIV.load(Ordering::Relaxed)) / mul
}

const fn gcd(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        let t = a % b;
        a = b;
        b = t;
    }
    a
}

/// The preemption tick in ticks: 10 ms of the platform's timebase (was the
/// literal `100_000` cycles, which is 10 ms only at 10 MHz).
pub fn default_tick_cycles() -> u64 {
    ns_to_ticks(10_000_000)
}

/// Arm this hart's timer at an absolute tick count.
pub fn arm_timer_ticks(deadline: u64) {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        if timer_uses_sstc() {
            riscv::write_csr_stimecmp(CSR_STIMECMP, deadline);
        } else {
            sbi_rt::set_timer(deadline);
        }
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        let _ = deadline;
    }
}

/// Write one console register.
#[inline]
fn uart_write_reg(base: usize, reg: usize, value: u8) {
    let addr = crate::phys::phys_to_virt(base) + (reg << UART_SHIFT.load(Ordering::Relaxed));
    // SAFETY: `base` is the console window the tree named — physical before
    // the switch, through the direct map after it (RFC-0098 C4); the width
    // matches the node's `reg-io-width`.
    unsafe {
        if UART_WIDTH.load(Ordering::Relaxed) == 4 {
            core::ptr::write_volatile(addr as *mut u32, u32::from(value));
        } else {
            core::ptr::write_volatile(addr as *mut u8, value);
        }
    }
}

#[inline]
fn uart_read_reg(base: usize, reg: usize) -> u8 {
    let addr = crate::phys::phys_to_virt(base) + (reg << UART_SHIFT.load(Ordering::Relaxed));
    // SAFETY: as above.
    unsafe {
        if UART_WIDTH.load(Ordering::Relaxed) == 4 {
            (core::ptr::read_volatile(addr as *const u32) & 0xff) as u8
        } else {
            core::ptr::read_volatile(addr as *const u8)
        }
    }
}

/// Emit one byte on the console: wait for the transmitter, then write. Dropped
/// when no console is known yet (never a write to a guessed address). Every byte
/// is first kept in the console ring (RFC-0107 Phase 2) — the one hook every
/// console path passes, so the ring holds what the UART receives, in its order,
/// and also what was written before the console was known. The line the byte
/// belongs to is this hart's from its first byte to `\n` (`console_line`,
/// TASK-0327B P4 H0d): on a multi-hart board two writers otherwise interleave
/// byte by byte and no marker survives.
pub fn console_write_byte(byte: u8) {
    super::console_line::with_line(byte, console_emit);
}

/// Sends this hart's gathered partial line now (the panic path's first act, so the trace
/// ends at the dying hart's last byte).
pub fn console_flush_line() {
    super::console_line::flush_current_hart(console_emit);
}

/// One byte to the ring and the UART, in that order, with no line discipline: the leaf every
/// console path ends in.
fn console_emit(byte: u8) {
    super::console_ring::record_then(byte, |byte| {
        let base = UART_BASE.load(Ordering::Acquire);
        if base == 0 {
            return;
        }
        // TASK-0260B P3: the wait for the transmitter is bounded (2 ms, twenty byte
        // times at the slowest console rate) — a transmitter that stops, whatever stops
        // it, must not stop the kernel. The byte then stays in the ring only and the stall
        // is counted (`uart_stalls`, stamped into the boot milestones).
        let bound = ns_to_ticks(2_000_000);
        let t0 = crate::arch::riscv::read_time();
        while uart_read_reg(base, UART_LSR) & LSR_TX_IDLE == 0 {
            if bound != 0 && crate::arch::riscv::read_time().wrapping_sub(t0) > bound {
                UART_STALLS.fetch_add(1, Ordering::Relaxed);
                return;
            }
        }
        uart_write_reg(base, UART_TX, byte);
    });
}

/// Bytes the console gave up on because the transmitter never became ready in time.
static UART_STALLS: AtomicUsize = AtomicUsize::new(0);

pub fn uart_stalls() -> usize {
    UART_STALLS.load(Ordering::Relaxed)
}

/// Collection of HAL devices for the running machine.
pub struct Machine {
    timer: PlatformTimer,
    uart: PlatformUart,
    tlb: PlatformTlb,
    irq: PlatformIrq,
}

impl Machine {
    /// Constructs the HAL facade.
    pub const fn new() -> Self {
        Self { timer: PlatformTimer, uart: PlatformUart, tlb: PlatformTlb, irq: PlatformIrq }
    }

    pub const fn timer(&self) -> &PlatformTimer {
        &self.timer
    }

    pub const fn uart(&self) -> &PlatformUart {
        &self.uart
    }

    pub const fn tlb(&self) -> &PlatformTlb {
        &self.tlb
    }

    pub const fn irq(&self) -> &PlatformIrq {
        &self.irq
    }
}

/// The `time` CSR scaled by the tree's timebase.
pub struct PlatformTimer;

impl Timer for PlatformTimer {
    fn now(&self) -> u64 {
        ticks_to_ns(riscv::read_time())
    }

    fn set_wakeup(&self, deadline: u64) {
        arm_timer_ticks(ns_to_ticks(deadline));
    }
}

/// The console the tree named.
pub struct PlatformUart;

impl Uart for PlatformUart {
    fn write_byte(&self, byte: u8) {
        console_write_byte(byte);
    }
}

/// Trivial IRQ controller wrapper (source enable/disable live in `hal::plic`).
pub struct PlatformIrq;

impl IrqCtl for PlatformIrq {
    fn enable(&self, _irq: usize) {}
    fn disable(&self, _irq: usize) {}
}

/// Sv39 TLB helper issuing `sfence.vma` when compiled for RISC-V.
pub struct PlatformTlb;

impl Tlb for PlatformTlb {
    fn flush_all(&self) {
        #[cfg(target_arch = "riscv64")]
        unsafe {
            core::arch::asm!("sfence.vma x0, x0", options(nostack));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tick_conversions_are_exact_for_both_timebases() {
        for hz in [10_000_000u64, 24_000_000] {
            let g = gcd(1_000_000_000, hz);
            NS_MUL.store(1_000_000_000 / g, Ordering::Relaxed);
            NS_DIV.store(hz / g, Ordering::Relaxed);
            TIMEBASE_HZ.store(hz, Ordering::Relaxed);
            assert_eq!(ticks_to_ns(hz), 1_000_000_000);
            assert_eq!(ns_to_ticks(1_000_000_000), hz);
            assert_eq!(ns_to_ticks(10_000_000), hz / 100);
            assert_eq!(default_tick_cycles(), hz / 100);
            assert_eq!(ticks_per_us(), hz / 1_000_000);
        }
        NS_DIV.store(0, Ordering::Relaxed);
        NS_MUL.store(0, Ordering::Relaxed);
        assert_eq!(ticks_to_ns(12345), 0);
        assert_eq!(ns_to_ticks(12345), 0);
        assert_eq!(gcd(1_000_000_000, 24_000_000), 8_000_000);
    }

    #[test]
    fn console_bytes_are_dropped_before_init() {
        UART_BASE.store(0, Ordering::Relaxed);
        console_write_byte(b'x'); // must not touch memory
        assert!(uart_window().is_none());
        assert!(plic_s_context(0).is_none());
    }
}
