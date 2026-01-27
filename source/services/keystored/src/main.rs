// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Keystored daemon entrypoint wiring default transport to service logic
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: No tests
//! ADR: docs/adr/0017-service-architecture.md

#![cfg_attr(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"), no_std, no_main)]

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
nexus_service_entry::declare_entry!(os_entry);

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
fn os_entry() -> keystored::LiteResult<()> {
    keystored::service_main_loop(keystored::ReadyNotifier::new(|| {}))
}

#[cfg(nexus_env = "host")]
fn main() {
    keystored::daemon_main(|| {});
}
