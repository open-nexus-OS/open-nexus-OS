// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: ingressd entrypoint. Host: explains the build-time exposure
//! table (what this policy lets the gateway front). The OS-lite entry —
//! facade/policyd slots wired by init, the accept/forward loop, the
//! behaviour markers — lands with TASK-0052 P3; until then the crate carries
//! no `nexus-service` metadata and is never embedded.
//! OWNERS: @runtime @security
//! STATUS: Experimental
//! TEST_COVERAGE: tests/ingress_host/

#[cfg(not(any(nexus_env = "host", nexus_env = "os")))]
compile_error!(
    "nexus_env: missing. Set RUSTFLAGS='--cfg nexus_env=\"host\"' or '--cfg nexus_env=\"os\"'.",
);

#[cfg(nexus_env = "host")]
fn main() {
    print!("{}", ingressd::host_explain());
}

#[cfg(nexus_env = "os")]
fn main() {
    // P3 replaces this with the OS-lite service loop (declare_entry!).
    loop {
        core::hint::spin_loop();
    }
}
