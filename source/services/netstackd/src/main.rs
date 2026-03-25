// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(
    all(nexus_env = "os", target_arch = "riscv64", target_os = "none", feature = "os-lite"),
    no_std,
    no_main
)]

//! CONTEXT: netstackd (v0) — networking owner service for OS bring-up
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Proven via QEMU markers (TASK-0003..0005 / scripts/qemu-test.sh + tools/os2vm.sh)
//! ADR: docs/adr/0005-dsoftbus-architecture.md
//!
//! Responsibilities (v0, Step 1):
//! - Own virtio-net + smoltcp via `userspace/nexus-net-os`.
//! - Prove the facade can do real on-wire traffic (gateway ping + UDP DNS).
//! - Export a minimal sockets facade via IPC for other services (TASK-0003).

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none", feature = "os-lite"))]
extern crate alloc;

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none", feature = "os-lite"))]
mod os;

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none", feature = "os-lite"))]
nexus_service_entry::declare_entry!(os_entry);

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none", feature = "os-lite"))]
fn os_entry() -> core::result::Result<(), ()> {
    crate::os::entry::emit_ready_marker();
    let crate::os::bootstrap::BootstrapResult { net, bind_ip: _bind_ip } =
        crate::os::bootstrap::bootstrap_network();
    crate::os::facade::runtime::run_facade_loop(net);
}

#[cfg(not(all(
    nexus_env = "os",
    target_arch = "riscv64",
    target_os = "none",
    feature = "os-lite"
)))]
fn main() -> ! {
    // Host builds intentionally do nothing for now.
    loop {
        core::hint::spin_loop();
    }
}
