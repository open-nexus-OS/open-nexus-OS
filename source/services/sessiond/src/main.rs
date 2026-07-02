// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: sessiond daemon entry point — owner of the `session-start` boot stage (RFC-0069 §4)
//! and the session/login authority (TASK-0065B).
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU marker ladder (`sessiond: ready`, `sessiond: greeter (n=…)` /
//! `sessiond: session start (user=… product=…)`).
//! RFC: docs/rfcs/RFC-0069-init-declarative-service-manifest-slot-discipline-boot-stages.md

#![forbid(unsafe_code)]
#![cfg_attr(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"), no_std, no_main)]

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
nexus_service_entry::declare_entry!(os_entry);

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
fn os_entry() -> sessiond::SessiondResult<()> {
    sessiond::service_main_loop(sessiond::ReadyNotifier::new(|| {}))
}

#[cfg(nexus_env = "host")]
fn main() {
    println!("sessiond: host mode - use crate tests");
}
