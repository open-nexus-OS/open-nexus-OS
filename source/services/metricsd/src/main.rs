// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: metricsd daemon entrypoint wiring os-lite runtime backend
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Host unit tests live in `src/lib.rs`
//! ADR: docs/adr/0017-service-architecture.md

#![forbid(unsafe_code)]
#![cfg_attr(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"), no_std, no_main)]

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
nexus_service_entry::declare_entry!(os_entry);

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
fn os_entry() -> metricsd::MetricsResult<()> {
    metricsd::service_main_loop(metricsd::ReadyNotifier::new(|| {}))
}

#[cfg(nexus_env = "host")]
fn main() {
    println!("metricsd: host mode - use crate tests");
}
