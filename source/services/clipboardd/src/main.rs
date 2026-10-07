// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: clipboardd entry point — the clipboard authority (RFC-0094). The
//! bare-metal target runs the service loop; host builds carry the library half.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: tests/contract.rs (host); QEMU `clipboardd: ready` in every profile

#![forbid(unsafe_code)]
#![cfg_attr(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"), no_std, no_main)]

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
nexus_service_entry::declare_entry!(os_entry);

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
fn os_entry() -> clipboardd::os_lite::Result<()> {
    clipboardd::os_lite::service_main_loop()
}

#[cfg(not(all(nexus_env = "os", target_arch = "riscv64", target_os = "none")))]
fn main() {
    println!("clipboardd: host mode - the service loop runs on the OS target only");
}
