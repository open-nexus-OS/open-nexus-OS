// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: timed daemon entrypoint wiring for os-lite and host
//! OWNERS: @runtime
//! PUBLIC API: os_entry() (os-lite), main() (host stub)
//! DEPENDS_ON: timed::service_main_loop, nexus-service-entry (os-lite)

#![forbid(unsafe_code)]
#![cfg_attr(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"), no_std, no_main)]

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
nexus_service_entry::declare_entry!(os_entry);

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
fn os_entry() -> timed::TimedResult<()> {
    timed::service_main_loop(timed::ReadyNotifier::new(|| {}))
}

#[cfg(nexus_env = "host")]
fn main() {
    println!("timed: host mode - use crate tests");
}
