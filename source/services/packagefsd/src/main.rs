// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"), no_std, no_main)]

//! CONTEXT: Packagefsd entrypoint – wires host and OS-lite entrypoints into service loops
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Unit tests (std_server) + QEMU marker ladder (os_lite)
//! ADR: docs/adr/0009-bundle-manager-architecture.md

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
nexus_service_entry::declare_entry!(os_entry);

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
fn os_entry() -> packagefsd::LiteResult<()> {
    // RFC-0068: fold routine debug_println markers into one `packagefsd N/N` verdict; the ready
    // notifier fires the flush when the service signals ready (interactive boots; proof stays raw).
    nexus_abi::service_verdict_arm();
    packagefsd::service_main_loop(packagefsd::ReadyNotifier::new(|| {
        nexus_abi::service_verdict_flush("packagefsd");
    }))
}

#[cfg(not(all(nexus_env = "os", target_arch = "riscv64", target_os = "none")))]
fn main() -> ! {
    #[cfg(not(all(nexus_env = "os", feature = "os-lite")))]
    packagefsd::touch_schemas();
    if let Err(err) = packagefsd::service_main_loop(packagefsd::ReadyNotifier::new(|| ())) {
        eprintln!("packagefsd: {err}");
    }
    loop {
        core::hint::spin_loop();
    }
}
