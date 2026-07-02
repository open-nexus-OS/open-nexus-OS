// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: sessiond — the session authority owning the `session-start` boot
//! stage (RFC-0069 §4) and the session/login contract (TASK-0065B).
//! OWNERS: @runtime
//! STATUS: Functional (state machine + user registry + wire protocol; auth
//! docks later behind OP_LOGIN, `Locked` reserved)
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `cargo test -p sessiond` (state machine, user manifest);
//! QEMU marker ladder (`sessiond: ready`, `sessiond: greeter (n=…)` /
//! `sessiond: session start (user=… product=…)`).
//!
//! Authority split (ADR-0036 discipline): sessiond owns WHO exists and WHICH
//! session is active; SystemUI owns what that means visually (greeter config,
//! product → shell resolution); windowd renders and relays, never forges state.

#![forbid(unsafe_code)]
#![cfg_attr(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"), no_std)]

extern crate alloc;

pub mod state;
pub mod users;

#[cfg(all(nexus_env = "os", feature = "os-lite"))]
mod os_lite;
#[cfg(all(nexus_env = "os", feature = "os-lite"))]
pub use os_lite::{service_main_loop, ReadyNotifier, SessiondError, SessiondResult};

/// Host stub — the service loop is os-lite only; the session model and user
/// registry are host-tested via the `state`/`users` modules.
#[cfg(nexus_env = "host")]
pub fn run() {
    println!("sessiond: host mode - use crate tests");
}
