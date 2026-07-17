// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: pinched daemon entry point — system-internal compute broker.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: See lib.rs

#![forbid(unsafe_code)]
#![cfg_attr(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"), no_std, no_main)]

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
nexus_service_entry::declare_entry!(os_entry);

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
fn os_entry() -> pinched::PinchedResult<()> {
    pinched::service_main_loop(pinched::ReadyNotifier::new(|| {}))
}

#[cfg(nexus_env = "host")]
fn main() {
    println!("pinched: host mode - use library API for testing");
}
