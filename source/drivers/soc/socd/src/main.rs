// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: socd entry point — the SoC glue owner (RFC-0106). The bare-metal
//! target runs the service loop; host builds carry the library half only.
//! OWNERS: @runtime @kernel-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: tests/contract.rs (host); QEMU `socd: ready (…)` in every profile

#![forbid(unsafe_code)]
#![cfg_attr(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"), no_std, no_main)]

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
nexus_service_entry::declare_entry!(os_entry);

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
fn os_entry() -> socd::os_lite::Result<()> {
    socd::os_lite::service_main_loop()
}

#[cfg(not(all(nexus_env = "os", target_arch = "riscv64", target_os = "none")))]
fn main() {
    println!("socd: host mode - the service loop runs on the OS target only");
}
