// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"), no_std, no_main)]

//! CONTEXT: ingressd entrypoint. Host: explains the build-time exposure
//! table (what this policy lets the gateway front). OS-lite: the gateway
//! loop (`os_lite::service_main_loop`) — intents on the server slot,
//! policyd as authority, netstackd as the data plane.
//! OWNERS: @runtime @security
//! STATUS: Experimental
//! TEST_COVERAGE: tests/ingress_host/, QEMU `ingressd:` / `SELFTEST: ingress` markers

#[cfg(not(any(nexus_env = "host", nexus_env = "os")))]
compile_error!(
    "nexus_env: missing. Set RUSTFLAGS='--cfg nexus_env=\"host\"' or '--cfg nexus_env=\"os\"'.",
);

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
nexus_service_entry::declare_entry!(os_entry);

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
fn os_entry() -> Result<(), ingressd::os_lite::GatewayError> {
    ingressd::os_lite::service_main_loop()
}

#[cfg(nexus_env = "host")]
fn main() {
    print!("{}", ingressd::host_explain());
}

#[cfg(all(nexus_env = "os", not(all(target_arch = "riscv64", target_os = "none"))))]
fn main() {
    let _ = ingressd::os_lite::service_main_loop();
    loop {
        core::hint::spin_loop();
    }
}
