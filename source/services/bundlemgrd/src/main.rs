// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"), no_std, no_main)]

//! CONTEXT: Bundle manager daemon entrypoint – wires host and OS-lite entrypoints into service loops
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable (v1.0)
//! TEST_COVERAGE: 3 E2E tests + 11 host unit tests
//! ADR: docs/adr/0009-bundle-manager-architecture.md

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
nexus_service_entry::declare_entry!(os_entry);

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
fn os_entry() -> bundlemgrd::LiteResult<()> {
    bundlemgrd::service_main_loop(
        bundlemgrd::ReadyNotifier::new(|| {}),
        bundlemgrd::ArtifactStore::new(),
    )
}

#[cfg(not(all(nexus_env = "os", target_arch = "riscv64", target_os = "none")))]
fn main() -> ! {
    bundlemgrd::touch_schemas();
    let artifacts = bundlemgrd::ArtifactStore::new();
    if let Err(err) =
        bundlemgrd::service_main_loop(bundlemgrd::ReadyNotifier::new(|| ()), artifacts)
    {
        eprintln!("bundlemgrd: {err}");
    }
    loop {
        core::hint::spin_loop();
    }
}
