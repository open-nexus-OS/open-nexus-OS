// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The ONE unsafe-bearing nxboot module (ADR-0059): early entry
//! asm, polled 16550 uart output and the SBI reset. Everything here must
//! stay position-independent until self-relocation lands (A2) — the
//! firmware drops the payload at 0x8020_0000 while the link home is
//! 0x8E00_0000, so only PC-relative addressing (`la`, medany code model)
//! is legal on the early path.
//! OWNERS: @security @runtime
//! STATUS: Experimental (TASK-0289 Phase A)
//! TEST_COVERAGE: none (target-only; behavior proven via QEMU markers at A4)
//! ADR: docs/adr/0059-first-stage-boot-chain-nxboot-handoff.md

#![allow(unsafe_code)]

// Entry: park all but the boot flow on the stack the linker reserved,
// keep a0 (hartid) / a1 (DTB) untouched for the Rust entry and the later
// jump into the verified image. `.option norelax` guards the gp setup
// against linker relaxation rewriting it into a gp-relative bootstrap.
core::arch::global_asm!(
    r#"
    .section .text._start, "ax", @progbits
    .globl _start
    .align 4
_start:
    .option push
    .option norelax
    la   gp, __global_pointer$
    .option pop
    la   sp, __boot_stack_top
    j    nxboot_main
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
