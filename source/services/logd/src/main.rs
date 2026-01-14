// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
#![forbid(unsafe_code)]
#![cfg_attr(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"), no_std, no_main)]

//! CONTEXT: logd daemon entrypoint wiring default transport to shared service logic
//!
//! OWNERS: @runtime
//!
//! STATUS: Experimental
//!
//! API_STABILITY: Unstable
//!
//! TEST_COVERAGE: Host tests in `source/services/logd/tests/`
//!
//! ADR: docs/adr/0017-service-architecture.md

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
nexus_service_entry::declare_entry!(os_entry);

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
fn os_entry() -> logd::LiteResult<()> {
    logd::service_main_loop(logd::ReadyNotifier::new(|| {}))
}

#[cfg(not(all(nexus_env = "os", target_arch = "riscv64", target_os = "none")))]
fn main() -> ! {
    logd::touch_schemas();
    if let Err(err) = logd::service_main_loop(logd::ReadyNotifier::new(|| ())) {
        eprintln!("logd: {err}");
    }
    loop {
        core::hint::spin_loop();
    }
}
