// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(all(nexus_env = "os", target_arch = "riscv64", target_os = "none")), no_std)]
#![cfg_attr(
    not(all(nexus_env = "os", target_arch = "riscv64", target_os = "none")),
    forbid(unsafe_code)
)]
#![deny(clippy::all, missing_docs)]

//! CONTEXT: Tiny collection of executable payloads used by execd while the ELF/NXB loaders
//! OWNERS: @runtime
//! PUBLIC API: HELLO_ELF, hello_child_entry(), BootstrapMsg
//! DEPENDS_ON: nexus-abi (OS)
//! INVARIANTS: No MMIO; use debug_println syscall; stable UART markers
//! ADR: docs/adr/0001-runtime-roles-and-boundaries.md

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
    if ptr.is_null() {
        None
    } else {
        // SAFETY: the kernel passes a valid pointer to an immutable BootstrapMsg.
        unsafe { ptr.as_ref().copied() }
    }
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
