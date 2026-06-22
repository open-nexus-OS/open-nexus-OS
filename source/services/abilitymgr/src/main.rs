// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: abilitymgr daemon entry point — the ability-lifecycle broker.
//! OWNERS: @runtime @ui
//! STATUS: Functional
//! API_STABILITY: Unstable (v6b bring-up)
//! TEST_COVERAGE: See lib.rs (lifecycle + wire host tests) + tests/cli.rs.
//! ADR: docs/adr/0036-ability-lifecycle-vs-process-vs-registry-service-split.md

#![forbid(unsafe_code)]
#![cfg_attr(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"), no_std, no_main)]

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
nexus_service_entry::declare_entry!(os_entry);

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
fn os_entry() -> abilitymgr::AbilitymgrResult<()> {
    abilitymgr::service_main_loop(abilitymgr::ReadyNotifier::new(|| {}))
}

#[cfg(nexus_env = "host")]
fn main() {
    abilitymgr::run();
}
