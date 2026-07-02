// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: sessiond — the session manager owning the `session-start` boot stage (RFC-0069 §4).
//! OWNERS: @runtime
//! STATUS: Experimental (skeleton)
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU marker ladder; host = stub only.
//!
//! Today the skeleton starts the DEFAULT session immediately (the shell stays
//! windowd-hosted, zero visible change) — its markers pin the seam the login
//! track docks onto: the greeter/authentication later replaces the auto-start,
//! and the resolved user session selects the SystemUI shell profile.

#![forbid(unsafe_code)]
#![cfg_attr(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"), no_std)]

#[cfg(all(nexus_env = "os", feature = "os-lite"))]
mod os_lite;
#[cfg(all(nexus_env = "os", feature = "os-lite"))]
pub use os_lite::{service_main_loop, ReadyNotifier, SessiondError, SessiondResult};

/// Host stub — the service logic is os-lite only for now.
#[cfg(nexus_env = "host")]
pub fn run() {
    println!("sessiond: host mode - use crate tests");
}
