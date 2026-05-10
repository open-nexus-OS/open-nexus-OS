#![cfg_attr(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"), no_std, no_main)]
// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: updated daemon entrypoint wiring default transport to service logic

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
nexus_service_entry::declare_entry!(os_entry);

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
fn os_entry() -> updated::LiteResult<()> {
    updated::service_main_loop(updated::ReadyNotifier::new(|| {}))
}

#[cfg(nexus_env = "host")]
fn main() {
    updated::daemon_main(|| {});
}
