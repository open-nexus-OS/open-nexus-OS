// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Early boot routines for the NEURON microkernel
//! OWNERS: @kernel-boot-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: No tests (boot path proven via QEMU marker contract)
//! PUBLIC API: early_boot_init(hartid, dtb)
//! DEPENDS_ON: arch::riscv::clear_bss, trap::install_trap_vector, init_heap
//! INVARIANTS: Single-invocation; interrupts masked; minimal diagnostics on OS path
//! ADR: docs/adr/0001-runtime-roles-and-boundaries.md

#[cfg(not(test))]
extern "C" {
    static mut __bss_start: u8;
    static mut __bss_end: u8;
}

/// Perform the machine initialisation required before the kernel can run.
///
/// # Safety
///
/// This must only be invoked once on the boot CPU before any Rust code that
/// relies on initialised memory or traps executes. Callers must ensure the
/// stack is valid and interrupts are masked until setup completes.
pub fn early_boot_init(hartid: usize, dtb: usize) {
    // SAFETY: called once during early boot, before interrupts/threads.
    unsafe {
        zero_bss();
    }
    // RFC-0098 C3: the platform — console, PLIC, timebase — is built from the
    // tree in a1 BEFORE the first byte is logged; paging is off, so the tree is
    // read at its physical address. Without a valid tree there is no console and
    // the first log line below is dropped: silence on the harness, never a guess.
    crate::boot_fdt::record(hartid, dtb);
    let platform = crate::hal::platform::init_from_fdt(crate::boot_fdt::bytes());
    // Stage-policy: no heavy diagnostics in early boot on OS path.
    log_info!(target: "boot", "boot: ok");
    if let Err(e) = platform {
        log_info!(target: "boot", "boot: platform from fdt FAILED ({:?})", e);
    }

    // SAFETY: privileged context, trap vector install once.
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
