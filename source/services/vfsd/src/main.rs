// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"), no_std, no_main)]

//! CONTEXT: Vfsd entrypoint – wires host and OS-lite entrypoints into service loops
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Covered by selftest VFS phase
//! ADR: docs/adr/0017-service-architecture.md

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
nexus_service_entry::declare_entry!(os_entry);

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
fn os_entry() -> vfsd::Result<()> {
    vfsd::service_main_loop(vfsd::ReadyNotifier::new(|| {}))
}

#[cfg(not(all(nexus_env = "os", target_arch = "riscv64", target_os = "none")))]
fn main() {
    if let Err(err) = vfsd::service_main_loop(vfsd::ReadyNotifier::new(|| {})) {
        eprintln!("vfsd: exited with error: {err}");
        std::process::exit(1);
    }
}
