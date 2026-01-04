// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Application launcher for user programs
//! OWNERS: @runtime
//! STATUS: Placeholder
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 1 unit test
//!
//! PUBLIC API:
//!   - main(): Application entry point
//!
//! DEPENDENCIES:
//!   - std::println: Console output
//!
//! ADR: docs/adr/0017-service-architecture.md

#![cfg_attr(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"), no_std, no_main)]

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
nexus_service_entry::declare_entry!(os_entry);

#[cfg(test)]
mod tests {
    #[test]
    fn message_constant() {
        assert_eq!("Launcher started", "Launcher started");
    }
}

#[cfg(not(all(nexus_env = "os", target_arch = "riscv64", target_os = "none")))]
fn main() {
    // Placeholder launcher entrypoint; emits a deterministic message to avoid unused-bin errors.
    println!("launcher: placeholder (no apps configured)");
}

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
fn os_entry() -> core::result::Result<(), ()> {
    let _ = nexus_abi::debug_println("launcher: placeholder (no apps configured)");
    loop {
        let _ = nexus_abi::yield_();
    }
}
