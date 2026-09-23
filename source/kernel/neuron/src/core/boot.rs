// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Early boot routines for the NEURON microkernel — two phases around
//! the high-half switch (RFC-0098 C4, TASK-0286 P2). `early_boot_init` runs at
//! the load address with paging off: BSS, the tree, the platform, the boot
//! table; it returns the `satp` the wrapper writes before it jumps to the
//! high alias and re-applies the fixups. `high_boot_init` runs there: the
//! seam flips, traps and the timer are installed at their high addresses, the
//! heap is initialised with high pointers — nothing that holds a pointer is
//! built before the switch.
//! OWNERS: @kernel-boot-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: No tests (boot path proven via QEMU marker contract)
//! PUBLIC API: early_boot_init(hartid, dtb) -> satp, high_boot_init(satp)
//! DEPENDS_ON: arch::riscv::clear_bss, phys::build_boot_table, trap::install_trap_vector, init_heap, mm::frame_pool
//! INVARIANTS: Single-invocation; interrupts masked; minimal diagnostics on OS path
//! ADR: docs/adr/0001-runtime-roles-and-boundaries.md

#[cfg(not(test))]
extern "C" {
    static mut __bss_start: u8;
    static mut __bss_end: u8;
}

/// The switch table: one root page of 1 GiB leaves, zeroed with BSS.
#[link_section = ".bss.page_table"]
static mut BOOT_ROOT: crate::phys::BootRoot = crate::phys::BootRoot::new();

/// Phase one, at the load address with paging off. Returns the `satp` of the
/// boot table (0 when no table could be built — the wrapper then stays low and
/// the first log line says why).
///
/// # Safety
///
/// This must only be invoked once on the boot CPU before any Rust code that
/// relies on initialised memory or traps executes. Callers must ensure the
/// stack is valid and interrupts are masked until setup completes.
pub fn early_boot_init(hartid: usize, dtb: usize) -> usize {
    // SAFETY: called once during early boot, before interrupts/threads.
    unsafe {
        zero_bss();
    }
    // RFC-0098 C3: the platform — console, PLIC, timebase, memory banks — is
    // built from the tree in a1 BEFORE the first byte is logged; paging is off,
    // so the tree is read at its physical address. Without a valid tree there
    // is no console and the first log line below is dropped: silence on the
    // harness, never a guess.
    crate::boot_fdt::record(hartid, dtb);
    let platform = crate::hal::platform::init_from_fdt(crate::boot_fdt::bytes());
    log_info!(target: "boot", "boot: ok");
    if let Err(e) = platform {
        log_info!(target: "boot", "boot: platform from fdt FAILED ({:?})", e);
        return 0;
    }
    // The boot table: every bank and the two device windows through the direct
    // map, the tree wherever the previous stage put it, and the identity of the
    // gigabyte this code runs in — for the instructions between the satp write
    // and the jump.
    let (image_pa, _) = crate::boot_image::range();
    let windows = [crate::hal::platform::uart_window(), crate::hal::platform::plic_window()];
    let tree = crate::boot_fdt::range().map(|(s, e)| (s, e - s));
    let ranges =
        crate::hal::platform::memory_banks().chain(windows.into_iter().flatten()).chain(tree);
    // SAFETY: the static is written once here, by the boot hart, before any
    // other hart exists; its address is physical in this phase.
    let root = unsafe { &mut *core::ptr::addr_of_mut!(BOOT_ROOT) };
    if !crate::phys::build_boot_table(root, ranges, image_pa) {
        log_info!(target: "boot", "boot: memory beyond the direct map — staying low");
        return 0;
    }
    crate::phys::satp_for_root(core::ptr::addr_of!(BOOT_ROOT) as usize)
}

/// Phase two, in the high half with the fixups re-applied.
pub fn high_boot_init(boot_satp: usize) {
    crate::phys::set_paging_on(boot_satp);
    // SAFETY: privileged context, trap vector install once — at its high address.
    unsafe {
        crate::trap::install_trap_vector();
        // Arm first tick only when timer IRQs are enabled; default bring-up runs without timer
        // preemption to simplify early sequencing.
        #[cfg(feature = "timer_irq")]
        crate::trap::timer_arm(crate::trap::default_tick_cycles());
    }
    log_info!(target: "boot", "traps: ok");

    log_debug!(target: "boot", "A: before heap init");
    crate::init_heap();
    log_debug!(target: "boot", "B: after heap init");
    // The frame pool (RFC-0098 C4, TASK-0286): every bank the tree names minus
    // what is reserved or still owned elsewhere; the first page table takes
    // its frames from here, so it exists before the kernel address space.
    match crate::mm::frame_pool::init_from_tree() {
        Ok(s) => log_info!(target: "boot",
            "KINIT: mm frames (banks={} total={} free={} reserved={} excluded={})",
            s.banks, s.total, s.free, s.reserved, s.excluded),
        Err(e) => {
            log_error!(target: "boot", "KINIT: mm frames FAILED ({:?}) — no backing for page tables", e)
        }
    }
    let (base, end) = crate::boot_image::range();
    log_info!(target: "boot", "KINIT: kernel high half (base=0x{:x} load=0x{:x} len=0x{:x})",
        base, crate::phys::virt_to_phys(base), end - base);
    log_info!(target: "boot", "boot: returning to wrapper");
}

unsafe fn zero_bss() {
    #[cfg(not(test))]
    {
        crate::arch::riscv::clear_bss(
            core::ptr::addr_of_mut!(__bss_start),
            core::ptr::addr_of_mut!(__bss_end),
        );
    }
}
