// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
#![forbid(unsafe_code)]
#![cfg_attr(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"), no_std, no_main)]

//! CONTEXT: bootctld daemon entrypoint wiring default transport to shared
//! service logic (single boot-state authority, ADR-0055).
//! OWNERS: @reliability @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Host tests in `source/services/bootctld/tests/`
//! ADR: docs/adr/0055-bootctld-single-boot-state-authority.md

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
nexus_service_entry::declare_entry!(os_entry);

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
fn os_entry() -> bootctld::LiteResult<()> {
    bootctld::service_main_loop(bootctld::ReadyNotifier::new(|| {}))
}

#[cfg(not(all(nexus_env = "os", target_arch = "riscv64", target_os = "none")))]
fn main() -> ! {
    bootctld::touch_schemas();
    if let Err(err) = bootctld::service_main_loop(bootctld::ReadyNotifier::new(|| ())) {
        eprintln!("bootctld: {err}");
    }
    loop {
        core::hint::spin_loop();
    }
}
