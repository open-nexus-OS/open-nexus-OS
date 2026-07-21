// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: imed daemon entrypoint wiring for os-lite and host (RFC-0075).
//! OWNERS: @ui
//! PUBLIC API: os_entry() (os-lite), main() (host stub)
//! DEPENDS_ON: imed::os_lite::service_main_loop, nexus-service-entry (os-lite)

#![forbid(unsafe_code)]
#![cfg_attr(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"), no_std, no_main)]

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
nexus_service_entry::declare_entry!(os_entry);

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
fn os_entry() -> Result<(), imed::os_lite::ImedError> {
    imed::os_lite::service_main_loop()
}

#[cfg(not(all(nexus_env = "os", target_arch = "riscv64", target_os = "none")))]
fn main() {
    println!("imed: host mode - use crate tests");
}
