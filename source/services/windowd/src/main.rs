// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"), no_std, no_main)]
#![forbid(unsafe_code)]

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
nexus_service_entry::declare_entry!(os_entry);

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
fn os_entry() -> Result<(), nexus_abi::AbiError> {
    windowd::service_main_loop().map_err(|_| nexus_abi::AbiError::InvalidArgument)
}

#[cfg(not(all(nexus_env = "os", target_arch = "riscv64", target_os = "none")))]
fn main() {
    windowd::run();
}
