// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![no_std]
#![forbid(unsafe_code)]
#![deny(clippy::all, missing_docs)]

//! CONTEXT: Executable payloads for system testing and bootstrap
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: No tests
//!
//! PUBLIC API:
//!   - HELLO_ELF: Prebuilt ELF64/RISC-V binary
//!   - HELLO_MANIFEST: Manifest data
//!   - hello_child_entry: Child process entry point
//!
//! DEPENDENCIES:
//!   - nexus-abi: Kernel syscalls (OS builds)
//!   - core::hint::spin_loop: CPU spin loop
//!
//! ADR: docs/adr/0007-executable-payloads-architecture.md

/// Prebuilt ELF64/RISC-V payload that prints a UART marker and yields.
pub mod hello_elf;
pub use hello_elf::{HELLO_ELF, HELLO_MANIFEST, HELLO_MANIFEST_TOML};

use core::hint::spin_loop;

/// Bootstrap message delivered to the child task after [`nexus_abi::spawn`]
/// succeeds.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BootstrapMsg {
    /// Number of arguments supplied to the child. Zero for the MVP path.
    pub argc: u32,
    /// Pointer to the argv table in the child's address space. Zero for the MVP.
    pub argv_ptr: u64,
    /// Pointer to the environment table in the child's address space. Zero for the MVP.
    pub env_ptr: u64,
    /// Capability handle for the initial endpoint granted to the child.
    pub cap_seed_ep: u32,
    /// Reserved for future expansion.
    pub flags: u32,
}

/// Entry point invoked by the kernel once the child task is scheduled.
pub extern "C" fn hello_child_entry(bootstrap: *const BootstrapMsg) -> ! {
    let _ = read_bootstrap_once(bootstrap);
    log_line("child: hello-elf");

    loop {
        #[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
        {
            if nexus_abi::yield_().is_err() {
                spin_loop();
            }
        }
        #[cfg(not(all(nexus_env = "os", target_arch = "riscv64", target_os = "none")))]
        {
            spin_loop();
        }
    }
}

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
fn read_bootstrap_once(ptr: *const BootstrapMsg) -> Option<BootstrapMsg> {
    // Keep exec-payloads `unsafe_code`-free: the bootstrap message is optional
    // and not required for the marker-based hello payload.
    let _ = ptr;
    None
}

#[cfg(not(all(nexus_env = "os", target_arch = "riscv64", target_os = "none")))]
fn read_bootstrap_once(_ptr: *const BootstrapMsg) -> Option<BootstrapMsg> {
    None
}

fn log_line(line: &str) {
    #[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
    let _ = nexus_abi::debug_println(line);

    #[cfg(not(all(nexus_env = "os", target_arch = "riscv64", target_os = "none")))]
    let _ = line;
}

// MMIO UART helpers replaced by kernel debug_println syscall
