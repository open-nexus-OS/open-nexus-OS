// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"), no_std, no_main)]

//! CONTEXT: SAMGR daemon entrypoint – wires host and OS-lite entrypoints into service loops
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable (v1.0)
//! TEST_COVERAGE: 2 E2E tests (tests/e2e/samgrd_roundtrip.rs)
//! ADR: docs/adr/0017-service-architecture.md

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
nexus_service_entry::declare_entry!(os_entry);

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
fn os_entry() -> samgrd::LiteResult<()> {
    samgrd::service_main_loop(samgrd::ReadyNotifier::new(|| {}))
}

#[cfg(not(all(nexus_env = "os", target_arch = "riscv64", target_os = "none")))]
fn main() -> ! {
    samgrd::touch_schemas();
    if let Err(err) = samgrd::service_main_loop(samgrd::ReadyNotifier::new(|| ())) {
        eprintln!("samgrd: {err}");
    }
    loop {
        core::hint::spin_loop();
    }
}
