// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: settingsd daemon entry point — the typed settings registry service
//! (TASK-0072 Phase 8). On OS it serves the `nexus_abi::settingsd` wire
//! protocol with statefsd-backed persistence; on host it runs the legacy CLI.
//! OWNERS: @runtime
//! STATUS: Experimental
//! ADR: docs/adr/0011-settings-architecture.md

#![forbid(unsafe_code)]
#![cfg_attr(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"), no_std, no_main)]

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
nexus_service_entry::declare_entry!(os_entry);

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
fn os_entry() -> settingsd::SettingsdResult<()> {
    settingsd::service_main_loop()
}

#[cfg(all(nexus_env = "host", feature = "std"))]
fn main() {
    settingsd::run();
    println!("settingsd: ready");
}

#[cfg(all(nexus_env = "host", not(feature = "std")))]
fn main() {
    println!("settingsd: host mode - use crate tests");
}
