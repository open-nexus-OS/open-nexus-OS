// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The ONE unsafe-bearing nxboot module (ADR-0059): entry asm with
//! self-relocation (payload lands at 0x8020_0000, link home is 0x9200_0000),
//! bss/stack bring-up, polled 16550 uart, volatile MMIO accessors for the
//! virtio reader, the bounded bump allocator, the handoff-page write, the
//! icache-fenced jump into the verified image and the SBI reset. Everything
//! protocol-shaped lives OUTSIDE this file in `#![deny(unsafe_code)]` land
//! and reaches memory only through these bounded helpers.
//! OWNERS: @security @runtime
//! STATUS: Experimental (TASK-0289 Phase A)
//! TEST_COVERAGE: none (target-only; behavior proven via QEMU markers at A4)
//! ADR: docs/adr/0059-first-stage-boot-chain-nxboot-handoff.md

#![allow(unsafe_code)]

use core::alloc::{GlobalAlloc, Layout};
use core::sync::atomic::{AtomicUsize, Ordering};

// Entry: normalize the boot hart to hart 0 (lottery), self-relocate to the
// link home, then bring up gp/sp/bss and enter Rust. Runs position-independent (PC-relative `la` only) until the
// computed absolute jump to the relocated copy. a0 (hartid) / a1 (DTB) are
// never touched. `.option norelax` guards the gp setup against linker
// relaxation rewriting it into a gp-relative bootstrap.
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
    # PC-relative base (where we actually run) vs. the link home (ADR-0059).
    la   t0, __image_start
    li   t1, 0x92000000
    beq  t0, t1, 2f
    # Copy the loadable bytes home (doubleword strides, 8-aligned bounds).
    la   t2, __load_end
1:  bgeu t0, t2, 11f
    ld   t3, 0(t0)
    sd   t3, 0(t1)
    addi t0, t0, 8
    addi t1, t1, 8
    j    1b
11: fence.i
    # Absolute jump to the relocated continuation: home + (label - base).
    la   t4, 2f
    la   t5, __image_start
    sub  t4, t4, t5
    li   t6, 0x92000000
    add  t4, t4, t6
    jr   t4
    # Executing at the link home from here on.
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

const UART0_BASE: usize = 0x1000_0000;
const UART_LSR: usize = 0x5;
const LSR_TX_IDLE: u8 = 1 << 5;

/// Polled byte-wise uart write (pre-OS: no interrupts, no ownership
/// conflicts — the loader runs strictly before any driver exists).
pub fn uart_puts(msg: &str) {
    for byte in msg.bytes() {
        unsafe {
            let lsr = (UART0_BASE + UART_LSR) as *const u8;
            while core::ptr::read_volatile(lsr) & LSR_TX_IDLE == 0 {}
            core::ptr::write_volatile(UART0_BASE as *mut u8, byte);
        }
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

// ---- load region / handoff / jump ---------------------------------------

/// Where the verified image is assembled (kernel entry contract) and the
/// cap on how much a descriptor may claim (slot budget, RFC-0089 §2).
pub const LOAD_BASE: usize = 0x8020_0000;
pub const LOAD_MAX: usize = 56 * 1024 * 1024;

/// The destination window for the image copy. Exclusive to the loader:
/// pre-OS there is no other owner of this RAM (SBI is PMP-protected below
/// 0x8020_0000; the loader itself lives at 0x9200_0000).
pub fn load_region() -> &'static mut [u8] {
    unsafe { core::slice::from_raw_parts_mut(LOAD_BASE as *mut u8, LOAD_MAX) }
}

/// Writes the measured-boot page to its ADR-0059 address (volatile — the
/// kernel reads it after the jump, outside this program's dataflow).
pub fn write_handoff(page: &[u8; bootfmt::handoff::PAGE]) {
    let base = bootfmt::handoff::ADDR;
    for (i, &byte) in page.iter().enumerate() {
        unsafe { core::ptr::write_volatile((base + i) as *mut u8, byte) };
    }
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
