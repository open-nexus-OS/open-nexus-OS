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
//!   - uart_write_byte(): byte output through the kernel console syscall
//!
//! DEPENDENCIES:
//!   - nexus-init: Init library with backends
//!   - core::hint::spin_loop: CPU spin loop
//!
//! ADR: docs/adr/0017-service-architecture.md

#![forbid(unsafe_code)]
#![deny(clippy::all, missing_docs)]
#![allow(unexpected_cfgs)]

#[cfg(feature = "std-server")]
use nexus_init::touch_schemas;
#[cfg(any(feature = "std-server", feature = "os-lite"))]
use nexus_init::{service_main_loop, ReadyNotifier};

/// Entrypoint for the init binary. Delegates to the selected backend and keeps
/// the process alive once service bootstrapping finishes.
#[cfg(any(feature = "std-server", feature = "os-lite"))]
fn main() -> ! {
    #[cfg(all(feature = "std-server", not(all(nexus_env = "os", feature = "os-lite"))))]
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

#[cfg(all(nexus_env = "os", feature = "os-lite", target_arch = "riscv64", target_os = "none"))]
fn uart_println(s: &str) {
    for b in s.as_bytes() {
        uart_write_byte(*b);
    }
    uart_write_byte(b'\n');
}

#[cfg(all(nexus_env = "os", feature = "os-lite", target_arch = "riscv64", target_os = "none"))]
fn uart_write_byte(byte: u8) {
    // Through the kernel's console (RFC-0098 C3): init knows no UART address.
    let _ = nexus_abi::debug_putc(byte);
}
