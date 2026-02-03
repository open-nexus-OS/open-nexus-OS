// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
#![forbid(unsafe_code)]
#![cfg_attr(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"), no_std, no_main)]

//! CONTEXT: statefsd daemon entrypoint wiring default transport to service loop
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: No tests (bring-up only)
//! ADR: docs/adr/0023-statefs-persistence-architecture.md

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
nexus_service_entry::declare_entry!(os_entry);

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
fn os_entry() -> statefsd::LiteResult<()> {
    statefsd::service_main_loop(statefsd::ReadyNotifier::new(|| {}))
}

#[cfg(not(all(nexus_env = "os", target_arch = "riscv64", target_os = "none")))]
fn main() -> ! {
    statefsd::touch_schemas();
    if let Err(err) = statefsd::service_main_loop(statefsd::ReadyNotifier::new(|| ())) {
        eprintln!("statefsd: {err}");
    }
    loop {
        core::hint::spin_loop();
    }
}
