// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Early boot routines for the NEURON microkernel
//! OWNERS: @kernel-boot-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: No tests (boot path proven via QEMU marker contract)
//! PUBLIC API: early_boot_init()
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
pub fn early_boot_init() {
    // SAFETY: called once during early boot, before interrupts/threads.
    unsafe {
        zero_bss();
    }
    // Stage-policy: no heavy diagnostics in early boot on OS path.
    log_info!(target: "boot", "boot: ok");

    // SAFETY: privileged context, trap vector install once.
    unsafe {
        crate::trap::install_trap_vector();
        // Arm first tick only when timer IRQs are enabled; default bring-up runs without timer
        // preemption to simplify early sequencing.
        #[cfg(feature = "timer_irq")]
        crate::trap::timer_arm(crate::trap::DEFAULT_TICK_CYCLES);
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
