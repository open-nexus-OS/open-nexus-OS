// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: xhcid entry point — the USB host controller service (RFC-0099). The bare-metal
//! target runs the service loop; host builds carry the library half only.
//! OWNERS: @runtime @drivers
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: tests/xhci (host, against xhcid-model); QEMU the `usb` lane

#![forbid(unsafe_code)]
#![cfg_attr(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"), no_std, no_main)]

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
nexus_service_entry::declare_entry!(os_entry);

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
fn os_entry() -> xhcid::os_lite::Result<()> {
    xhcid::os_lite::service_main_loop()
}

#[cfg(not(all(nexus_env = "os", target_arch = "riscv64", target_os = "none")))]
fn main() {
    println!("xhcid: host mode - the service loop runs on the OS target only");
}
