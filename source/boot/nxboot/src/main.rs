// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: nxboot binary shell (TASK-0289 Phase A). On the bare-metal
//! riscv target this is the first-stage loader entry (A1: honest
//! unwired skeleton — it PANICS loudly and SBI-resets if ever executed;
//! the real BSB→verify→jump path lands with A2, the boot flip with A4).
//! On host targets it is an inert stub so the workspace builds/lints
//! uniformly. Machine logic lives in the host-tested library half.
//! OWNERS: @security @runtime
//! STATUS: Experimental
//! TEST_COVERAGE: library tests (select/trust); QEMU proof arrives at A4
//! ADR: docs/adr/0059-first-stage-boot-chain-nxboot-handoff.md

#![cfg_attr(all(target_arch = "riscv64", target_os = "none"), no_std, no_main)]
// The single unsafe allowance lives in `arch` (ADR-0059: one bounded
// early-asm/MMIO module); everything else denies unsafe.
#![deny(unsafe_code)]

#[cfg(all(target_arch = "riscv64", target_os = "none"))]
mod arch;

#[cfg(all(target_arch = "riscv64", target_os = "none"))]
mod boot {
    use crate::arch;

    /// Rust-side entry, reached from the `_start` asm (via the `no_mangle`
    /// export in `arch`) with the firmware registers (a0 = hartid,
    /// a1 = DTB) intact.
    ///
    /// A1 skeleton: nothing is wired to boot through this binary yet, so
    /// running it can only mean a harness misconfiguration — fail LOUD and
    /// reset (wait-loop doctrine: never hang, never fake progress).
    pub fn run(_hartid: usize, _dtb: usize) -> ! {
        arch::uart_puts("nxboot: PANIC (skeleton not wired - boot flip lands with TASK-0289 A4)\n");
        arch::system_reset()
    }

    #[panic_handler]
    fn panic(_info: &core::panic::PanicInfo<'_>) -> ! {
        arch::uart_puts("nxboot: PANIC (rust panic)\n");
        arch::system_reset()
    }
}

#[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
fn main() {
    // Host stub: the loader only exists as a bare-metal artifact.
}
