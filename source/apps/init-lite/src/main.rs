#![cfg_attr(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"), no_std)]
#![cfg_attr(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"), no_main)]

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
use core::panic::PanicInfo;
#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
use nexus_abi as abi;

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
#[no_mangle]
#[link_section = ".text._start"]
pub extern "C" fn _start() -> ! {
    let _ = abi::debug_putc(b'!');
    let _ = abi::debug_println("init: start");
    let _ = abi::yield_();
    let _ = abi::debug_println("init: ready");
    loop {
        for _ in 0..1024 { core::hint::spin_loop(); }
        let _ = abi::yield_();
    }
}

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
#[panic_handler]
fn panic(_: &PanicInfo) -> ! { loop {} }

#[cfg(not(all(nexus_env = "os", target_arch = "riscv64", target_os = "none")))]
fn main() {}

// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Minimal init process for OS bootstrap testing
//! OWNERS: @runtime
//! STATUS: Deprecated
//! API_STABILITY: Unstable
//! TEST_COVERAGE: No tests
//!
//! PUBLIC API:
//!   - _start(): OS entry point
//!   - panic(): Panic handler
//!
//! DEPENDENCIES:
//!   - nexus-abi: Kernel syscalls
//!   - core::hint::spin_loop: CPU spin loop
//!
//! ADR: docs/adr/0017-service-architecture.md
