// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: IME daemon entrypoint — minimal bootstrap stub for TASK-0059.
//! INTENT: register with samgr, emit "imed: ready", await focus/text-input IPC.
//! READINESS: print "imed: ready"; register/heartbeat with samgr (stub).
//! TESTS: unit tests in lib.rs.

#![cfg_attr(
    all(nexus_env = "os", target_arch = "riscv64", target_os = "none"),
    no_std,
    no_main
)]
#![forbid(unsafe_code)]

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
nexus_service_entry::declare_entry!(os_entry);

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
fn os_entry() -> Result<(), nexus_abi::AbiError> {
    // Stub: register with samgr, emit ready marker
    // Full integration deferred to follow-up tasks
    Ok(())
}

#[cfg(not(all(nexus_env = "os", target_arch = "riscv64", target_os = "none")))]
fn main() {
    let svc = imed::ImedService::new();
    assert!(svc.ready);
    println!("{}", imed::ImedService::READY_MARKER);
}
