// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Init process for launching core services and emitting UART markers
//! OWNERS: @init-team @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: No tests
//!
//! PUBLIC API:
//!   - main(): Init process entry point
//!   - uart_println(): UART output for OS builds
//!   - uart_write_byte(): Low-level UART byte output
//!
//! DEPENDENCIES:
//!   - nexus-init: Init library with backends
//!   - core::hint::spin_loop: CPU spin loop
//!
//! ADR: docs/adr/0017-service-architecture.md

#![forbid(unsafe_code)]
#![deny(clippy::all, missing_docs)]
#![allow(unexpected_cfgs)]

#[cfg(any(feature = "std-server", feature = "os-lite"))]
use nexus_init::{service_main_loop, ReadyNotifier};
#[cfg(feature = "std-server")]
use nexus_init::touch_schemas;

/// Entrypoint for the init binary. Delegates to the selected backend and keeps
/// the process alive once service bootstrapping finishes.
#[cfg(any(feature = "std-server", feature = "os-lite"))]
fn main() -> ! {
    #[cfg(all(
        feature = "std-server",
        not(all(nexus_env = "os", feature = "os-lite"))
    ))]
    touch_schemas();

    #[cfg(all(nexus_env = "os", feature = "os-lite"))]
    {
        if let Err(_err) = service_main_loop(ReadyNotifier::new(|| ())) {
            #[cfg(all(target_arch = "riscv64", target_os = "none"))]
            uart_println("init: fail runtime");
            #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
            eprintln!("init: fail runtime");
        }
    }

    #[cfg(not(all(nexus_env = "os", feature = "os-lite")))]
    {
        if let Err(err) = service_main_loop(ReadyNotifier::new(|| ())) {
            eprintln!("init: fatal error: {err}");
        }
    }

    loop {
        core::hint::spin_loop();
    }
}

// If no backend feature is enabled (or `os-payload` is selected for library-only usage),
// provide a trivial stub so tooling can type-check the workspace under different cfg sets.
#[cfg(not(any(feature = "std-server", feature = "os-lite")))]
fn main() -> ! {
    loop {
        core::hint::spin_loop();
    }
}

#[cfg(all(
    nexus_env = "os",
    feature = "os-lite",
    target_arch = "riscv64",
    target_os = "none"
))]
fn uart_println(s: &str) {
    for b in s.as_bytes() {
        uart_write_byte(*b);
    }
    uart_write_byte(b'\n');
}

#[cfg(all(
    nexus_env = "os",
    feature = "os-lite",
    target_arch = "riscv64",
    target_os = "none"
))]
fn uart_write_byte(byte: u8) {
    const UART0_BASE: usize = 0x1000_0000;
    const UART_TX: usize = 0x0;
    const UART_LSR: usize = 0x5;
    const LSR_TX_IDLE: u8 = 1 << 5;
    unsafe {
        while core::ptr::read_volatile((UART0_BASE + UART_LSR) as *const u8) & LSR_TX_IDLE == 0 {}
        if byte == b'\n' {
            core::ptr::write_volatile((UART0_BASE + UART_TX) as *mut u8, b'\r');
            while core::ptr::read_volatile((UART0_BASE + UART_LSR) as *const u8) & LSR_TX_IDLE == 0
            {
            }
        }
        core::ptr::write_volatile((UART0_BASE + UART_TX) as *mut u8, byte);
    }
}
