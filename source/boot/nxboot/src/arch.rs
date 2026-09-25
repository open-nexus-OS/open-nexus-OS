// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The ONE unsafe-bearing nxboot module (ADR-0059): entry asm that
//! applies the image's own R_RISCV_RELATIVE table (a static PIE, RFC-0098
//! C6 — the loader runs wherever the firmware put it and never moves),
//! bss/stack bring-up, the polled 16550-class console whose registers the
//! device tree named (`platform::init`), volatile MMIO accessors for the
//! virtio reader, the bounded bump allocator, the icache-fenced jump into
//! the verified image and the SBI reset. Everything protocol-shaped lives
//! OUTSIDE this file in `#![deny(unsafe_code)]` land and reaches memory
//! only through these bounded helpers. No address is a constant here.
//! OWNERS: @security @runtime
//! STATUS: Functional
//! TEST_COVERAGE: none (target-only; behavior proven via QEMU markers)
//! ADR: docs/adr/0059-first-stage-boot-chain-nxboot-handoff.md

#![allow(unsafe_code)]

extern crate alloc;

use core::alloc::{GlobalAlloc, Layout};
use core::sync::atomic::{AtomicUsize, Ordering};

// Entry: normalize the boot hart to hart 0 (lottery), apply the fixup table
// for the address we run at, then bring up gp/sp/bss and enter Rust. Every
// address is PC-relative (`la`/`lla` under the static code model); a0
// (hartid) / a1 (DTB) are never touched. `.option norelax` guards the gp
// setup against linker relaxation rewriting it into a gp-relative bootstrap.
core::arch::global_asm!(
    r#"
    .section .text._start, "ax", @progbits
    .globl _start
    .align 4
_start:
    # Boot-hart normalization (hart lottery): OpenSBI on `virt` hands the
    # image to a random hart. The kernel's SMP contract is `cpu0 = hart 0`
    # (secondaries 1..N-1 are started via HSM); a foreign boot hart left
    # bring-up DEGRADED (the boot hart was "started" as its own secondary)
    # and the PLIC/IRQ plane on the wrong hart. So the winner starts hart 0
    # at this very entry (a0 = 0, a1 = DTB via the HSM opaque) and stops
    # itself — hart 0 boots exactly like a hart-0 lottery win. HSM: EID
    # 0x48534D, FID 0 = hart_start(hartid, addr, opaque), FID 1 = hart_stop.
    beqz a0, 0f
    mv   a2, a1
    la   a1, _start
    li   a0, 0
    li   a7, 0x48534D
    li   a6, 0
    ecall
    li   a7, 0x48534D
    li   a6, 1
    ecall
9:  wfi
    j    9b
0:
    # Static PIE: the link base is 0, so where we run IS the fixup delta.
    # Add it to every absolute word the linker listed (R_RISCV_RELATIVE,
    # type 3); any other entry type is skipped — the image has none by
    # construction (PC-relative code), and the kernel names its own.
    lla  t0, __image_start
    lla  t1, __rela_dyn_start
    lla  t2, __rela_dyn_end
1:  bgeu t1, t2, 2f
    ld   t3, 0(t1)
    ld   t4, 8(t1)
    ld   t5, 16(t1)
    addi t1, t1, 24
    li   t6, 3
    bne  t4, t6, 1b
    add  t3, t3, t0
    add  t5, t5, t0
    sd   t5, 0(t3)
    j    1b
2:  .option push
    .option norelax
    la   gp, __global_pointer$
    .option pop
    la   sp, __boot_stack_top
    # Zero bss (allocator arena, queue pages, statics).
    la   t0, __bss_start
    la   t1, __bss_end
3:  bgeu t0, t1, 4f
    sd   zero, 0(t0)
    addi t0, t0, 8
    j    3b
4:  j    nxboot_main
"#
);

/// Linker-visible Rust entry (`no_mangle` is an unsafe-code surface, so
/// the export lives in this module); immediately hands off to the safe
/// boot flow.
#[no_mangle]
extern "C" fn nxboot_main(hartid: usize, dtb: usize) -> ! {
    crate::boot::run(hartid, dtb)
}

// ---- console: the UART the tree named (RFC-0098 C3) ----------------------

/// Console registers: base, `reg-shift`, `reg-io-width` — set once by
/// `platform::init` from `/chosen/stdout-path`. Zero base = no console yet:
/// bytes are dropped, never written to a guessed address.
static UART_BASE: AtomicUsize = AtomicUsize::new(0);
static UART_SHIFT: AtomicUsize = AtomicUsize::new(0);
static UART_WIDTH: AtomicUsize = AtomicUsize::new(1);
const UART_TX: usize = 0;
const UART_LSR: usize = 5;
const LSR_TX_IDLE: u8 = 1 << 5;

/// Name the console (a 16550-class UART at `base`, registers `1 << shift`
/// bytes apart, `width` bytes wide). Both the byte-stride `ns16550a` and the
/// board's 4-byte-stride part are this one driver.
pub fn set_console(base: usize, shift: usize, width: usize) {
    UART_SHIFT.store(shift, Ordering::Relaxed);
    UART_WIDTH.store(if width == 4 { 4 } else { 1 }, Ordering::Relaxed);
    UART_BASE.store(base, Ordering::Release);
}

fn uart_reg(base: usize, reg: usize) -> usize {
    base + (reg << UART_SHIFT.load(Ordering::Relaxed))
}

fn uart_read(base: usize, reg: usize) -> u8 {
    let addr = uart_reg(base, reg);
    unsafe {
        if UART_WIDTH.load(Ordering::Relaxed) == 4 {
            (core::ptr::read_volatile(addr as *const u32) & 0xff) as u8
        } else {
            core::ptr::read_volatile(addr as *const u8)
        }
    }
}

fn uart_write(base: usize, reg: usize, value: u8) {
    let addr = uart_reg(base, reg);
    unsafe {
        if UART_WIDTH.load(Ordering::Relaxed) == 4 {
            core::ptr::write_volatile(addr as *mut u32, u32::from(value));
        } else {
            core::ptr::write_volatile(addr as *mut u8, value);
        }
    }
}

/// Polled byte-wise console write (pre-OS: no interrupts, no ownership
/// conflicts — the loader runs strictly before any driver exists).
pub fn uart_puts(msg: &str) {
    let base = UART_BASE.load(Ordering::Acquire);
    if base == 0 {
        return;
    }
    for byte in msg.bytes() {
        while uart_read(base, UART_LSR) & LSR_TX_IDLE == 0 {}
        uart_write(base, UART_TX, byte);
    }
}

/// SBI SRST cold reboot; if the SBI call returns (no SRST extension),
/// spin in `wfi` — visibly parked, never silently continuing.
pub fn system_reset() -> ! {
    let _ = sbi_rt::system_reset(sbi_rt::ColdReboot, sbi_rt::NoReason);
    loop {
        unsafe { core::arch::asm!("wfi") };
    }
}

/// The hart's `time` CSR: ticks at the tree's timebase (the loader's only clock — the SDHCI
/// reader's bounded waits, TASK-0246B).
pub fn time_ticks() -> u64 {
    let ticks: u64;
    unsafe { core::arch::asm!("rdtime {}", out(reg) ticks) };
    ticks
}

// ---- volatile accessors (the virtio module's only path to raw memory) ----

pub fn mmio_read32(addr: usize) -> u32 {
    unsafe { core::ptr::read_volatile(addr as *const u32) }
}

pub fn mmio_write32(addr: usize, value: u32) {
    unsafe { core::ptr::write_volatile(addr as *mut u32, value) }
}

pub fn read_u8(addr: usize) -> u8 {
    unsafe { core::ptr::read_volatile(addr as *const u8) }
}

pub fn write_u8(addr: usize, value: u8) {
    unsafe { core::ptr::write_volatile(addr as *mut u8, value) }
}

pub fn read_u16(addr: usize) -> u16 {
    unsafe { core::ptr::read_volatile(addr as *const u16) }
}

pub fn write_u16(addr: usize, value: u16) {
    unsafe { core::ptr::write_volatile(addr as *mut u16, value) }
}

pub fn write_u32(addr: usize, value: u32) {
    unsafe { core::ptr::write_volatile(addr as *mut u32, value) }
}

pub fn write_u64(addr: usize, value: u64) {
    unsafe { core::ptr::write_volatile(addr as *mut u64, value) }
}

/// Full memory fence (publish queue writes before the MMIO notify).
pub fn fence_rw() {
    core::sync::atomic::fence(Ordering::SeqCst);
}

// ---- device-shared static memory (virtqueue + request header page) ------

#[repr(C, align(4096))]
struct SharedPage([u8; 4096]);

/// One page for the legacy-layout virtqueue (desc + avail + used; PFN
/// register needs 4 KiB alignment).
static mut QUEUE_PAGE: SharedPage = SharedPage([0; 4096]);
/// One page for request headers + the status byte.
static mut REQ_PAGE: SharedPage = SharedPage([0; 4096]);

pub fn queue_page_addr() -> usize {
    core::ptr::addr_of!(QUEUE_PAGE) as usize
}

pub fn req_page_addr() -> usize {
    core::ptr::addr_of!(REQ_PAGE) as usize
}

// ---- image range / load region / jump -----------------------------------

/// The cap on how much a descriptor may claim (slot budget, RFC-0089 §2) =
/// the size of the kernel window the loader looks for.
pub const LOAD_MAX: usize = 56 * 1024 * 1024;

/// This image's own physical range `[start, end)` (text through the loader
/// stack), PC-relative from the linker symbols — the window search excludes it.
pub fn image_range() -> (usize, usize) {
    extern "C" {
        static __image_start: u8;
        static __image_end: u8;
    }
    unsafe { (&__image_start as *const u8 as usize, &__image_end as *const u8 as usize) }
}

/// The destination window for the image copy, chosen by `platform::kernel_window`
/// from the device tree: inside the first memory bank, clear of the reserved
/// ranges, this image and the tree. Exclusive to the loader pre-OS.
pub fn load_region(base: usize, len: usize) -> &'static mut [u8] {
    unsafe { core::slice::from_raw_parts_mut(base as *mut u8, len) }
}

/// A zeroed, page-aligned buffer from the loader's arena for the tree copy the
/// kernel receives: the kernel exposes it to init as pages (RFC-0098 C3), so
/// it must start on one. Never freed (one-shot program).
pub fn alloc_pages(len: usize) -> &'static mut [u8] {
    let bytes = len.div_ceil(4096) * 4096;
    let layout = match Layout::from_size_align(bytes, 4096) {
        Ok(l) => l,
        Err(_) => alloc_error_reset(),
    };
    let ptr = unsafe { alloc::alloc::alloc_zeroed(layout) };
    if ptr.is_null() {
        alloc_error_reset();
    }
    unsafe { core::slice::from_raw_parts_mut(ptr, bytes) }
}

fn alloc_error_reset() -> ! {
    uart_puts("nxboot: PANIC (alloc arena exhausted)\n");
    system_reset()
}

/// A read-only view of physical RAM the firmware handed us — the device tree in
/// `a1` (RFC-0098 C1). Pre-OS, paging is off and nothing else owns that RAM;
/// the caller has validated `addr`/`len` against the tree's own header first.
pub fn phys_slice(addr: usize, len: usize) -> &'static [u8] {
    unsafe { core::slice::from_raw_parts(addr as *const u8, len) }
}

/// Publishes the copied image to the instruction stream and jumps with the
/// firmware registers restored (a0 = hartid, a1 = DTB).
pub fn jump_kernel(load_addr: u64, hartid: usize, dtb: usize) -> ! {
    unsafe {
        core::arch::asm!(
            "fence.i",
            "jr {addr}",
            addr = in(reg) load_addr,
            in("a0") hartid,
            in("a1") dtb,
            options(noreturn),
        );
    }
}

// ---- bounded bump allocator (gpt Vec + dalek scratch + markers) ----------

const HEAP_LEN: usize = 192 * 1024;

#[repr(C, align(16))]
struct Heap([u8; HEAP_LEN]);

static mut HEAP: Heap = Heap([0; HEAP_LEN]);
static HEAP_CURSOR: AtomicUsize = AtomicUsize::new(0);

struct BumpAlloc;

unsafe impl GlobalAlloc for BumpAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let base = core::ptr::addr_of_mut!(HEAP) as usize;
        let align = layout.align().max(16);
        loop {
            let cur = HEAP_CURSOR.load(Ordering::Relaxed);
            let start = (base + cur + align - 1) & !(align - 1);
            let end = start + layout.size();
            if end > base + HEAP_LEN {
                return core::ptr::null_mut();
            }
            let next = end - base;
            if HEAP_CURSOR.compare_exchange(cur, next, Ordering::Relaxed, Ordering::Relaxed).is_ok()
            {
                return start as *mut u8;
            }
        }
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {
        // One-shot program: the whole arena dies at the jump.
    }
}

#[global_allocator]
static GLOBAL_ALLOC: BumpAlloc = BumpAlloc;

#[alloc_error_handler]
fn alloc_error(_layout: Layout) -> ! {
    uart_puts("nxboot: PANIC (alloc arena exhausted)\n");
    system_reset()
}
