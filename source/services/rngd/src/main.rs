// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: rngd daemon entry point — single entropy authority
//! OWNERS: @runtime @security
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: See lib.rs
//!
//! SECURITY INVARIANTS:
//!   - Entropy bytes MUST NOT be logged
//!   - All requests MUST be policy-gated

#![forbid(unsafe_code)]
#![cfg_attr(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"), no_std, no_main)]

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
nexus_service_entry::declare_entry!(os_entry);

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
fn os_entry() -> rngd::RngdResult<()> {
    rngd::service_main_loop(rngd::ReadyNotifier::new(|| {}))
}

#[cfg(nexus_env = "host")]
fn main() {
    println!("rngd: host mode - use library API for testing");
}
