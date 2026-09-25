// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: IPC runtime abstractions for cross-process communication
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: 3 unit tests
//!
//! PUBLIC API:
//!   - Client trait: Client-side IPC interface
//!   - Server trait: Server-side IPC interface
//!   - Wait enum: Wait behavior for operations
//!   - IpcError: IPC error types
//!
//! DEPENDENCIES:
//!   - std::sync::mpsc: Host backend channels
//!   - nexus-abi: OS backend syscalls
//!
//! ADR: docs/adr/0003-ipc-runtime-architecture.md

#![forbid(unsafe_code)]
#![deny(clippy::all, missing_docs)]
#![allow(unexpected_cfgs)]
#![cfg_attr(
    all(feature = "os-lite", nexus_env = "os", target_arch = "riscv64", target_os = "none"),
    no_std
)]

#[cfg(all(nexus_env = "os", feature = "os-lite"))]
extern crate alloc;

use core::fmt;

#[cfg(all(nexus_env = "os", feature = "os-lite"))]
use alloc::vec::Vec;
#[cfg(not(all(nexus_env = "os", feature = "os-lite")))]
use std::vec::Vec;

/// Result type returned by IPC operations.
pub type Result<T> = core::result::Result<T, IpcError>;

/// Behaviour of a blocking call. There is no clock-bound variant (RFC-0093 §7, TASK-0054C
/// P2-a): a wait ends when the operation completes or the peer dies — never on a timer. Pacing
/// is a kernel one-shot timer on a waitset, not a receive timeout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wait {
    /// Block until the operation completes.
    Blocking,
    /// Return immediately if no progress can be made.
    NonBlocking,
}

impl Wait {
    /// Returns `true` when the caller requested a non-blocking attempt.
    pub const fn is_non_blocking(self) -> bool {
        matches!(self, Self::NonBlocking)
    }
}

/// Errors produced by the IPC runtime.
///
/// This enum is marked `#[non_exhaustive]` so downstream crates can keep a future-proof fallback
/// arm without triggering `unreachable_patterns` warnings, while still surfacing rich diagnostics
/// for known variants.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IpcError {
    /// Operation could not progress without blocking.
    WouldBlock,
    /// The caller exceeded the requested timeout.
    Timeout,
    /// The opposite endpoint disconnected.
    Disconnected,
    /// The kernel could not allocate required resources (e.g. receiver cap table full).
    NoSpace,
    /// Kernel returned an IPC failure.
    Kernel(nexus_abi::IpcError),
    /// IPC is not available under the current build.
    Unsupported,
}

impl fmt::Display for IpcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WouldBlock => write!(f, "operation would block"),
            Self::Timeout => write!(f, "operation timed out"),
            Self::Disconnected => write!(f, "peer disconnected"),
            Self::NoSpace => write!(f, "ipc ran out of resources"),
            Self::Kernel(err) => write!(f, "kernel rejected ipc request: {err:?}"),
            Self::Unsupported => write!(f, "ipc not supported for this configuration"),
        }
    }
}

#[cfg(nexus_env = "host")]
impl std::error::Error for IpcError {}

impl From<nexus_abi::IpcError> for IpcError {
    fn from(err: nexus_abi::IpcError) -> Self {
        match err {
            nexus_abi::IpcError::NoSpace => Self::NoSpace,
            other => Self::Kernel(other),
        }
    }
}

/// Client side of an IPC channel sending requests and receiving replies.
pub trait Client {
    /// Sends a request frame to the server.
    fn send(&self, frame: &[u8], wait: Wait) -> Result<()>;

    /// Receives a response frame from the server.
    fn recv(&self, wait: Wait) -> Result<Vec<u8>>;
}

/// Server side of an IPC channel receiving requests and delivering replies.
pub trait Server {
    /// Receives the next request frame.
    fn recv(&self, wait: Wait) -> Result<Vec<u8>>;

    /// Sends a response frame back to the caller.
    fn send(&self, frame: &[u8], wait: Wait) -> Result<()>;
}

/// The VMO a sender armed for its next VMO op, keyed by kernel sender identity
/// (a message moves one cap, so a VMO op is ARM + op).
pub mod armed_vmo;
pub mod audit;
/// Deadline-bounded waits (transitional, TASK-0324 P7: shrinking to zero — request/reply
/// goes through `exchange`, which has no clock) and the fleet route ask.
pub mod budget;
/// The ONE request/reply exchange: no timeout — the reply or the peer's death ends the wait.
pub mod exchange;

/// Kernel timers on declared notify endpoints + waitsets (TASK-0054C P2-b): pacing and device
/// watchdogs without a receive deadline.
pub mod timer;

/// logd OS-lite v1 wire helpers (host-testable parsers).
pub mod logd_wire;

/// policyd v2/v3 wire helpers (host-testable parsers).
pub mod policyd_wire;

/// Capability namespace — typed SSOT for capability names (RFC-0066).
pub mod capabilities;

/// Reusable policyd capability-check client (RFC-0066): one delegated cap check.
pub mod policyd;

/// The ONE socd client (RFC-0106, TASK-0246 P4c): `BRING_UP` and `CLOCK_RATE` over a declared
/// route.
pub mod socd;

/// Typed circuit breaker for server recv loops (SMP robustness): #[must_use]
/// verdict so die-on-error loops cannot be written silently.
pub mod resilience;

/// The reply-channel type every [`exchange`] call takes, re-exported so a caller of the ONE
/// request/reply API needs only this crate (TASK-0054C P2-c).
#[cfg(all(nexus_env = "os", feature = "os-lite"))]
pub use nexus_service_topology::SlotPair;

#[cfg(all(nexus_env = "host", feature = "std"))]
mod host;
#[cfg(all(nexus_env = "host", feature = "std"))]
pub use host::{loopback_channel, LoopbackClient, LoopbackServer};

// The OS backend: kernel IPC v1 syscalls. There is exactly ONE (TASK-0054C P3b).
// Two more used to shadow these same public names — `os.rs` for `os` without
// `os-lite`, and `os_lite.rs` for `os-lite` without `kernel-ipc`, the last home
// of the 512-byte frame ceiling RFC-0096 calls out. Neither compiled in ANY
// build: a `compile_error!` in both still built the OS workspace, `just diag`
// under all three cfgs, the kernel, and `init-lite` — the shipped init ELF,
// whose graph resolves `nexus-ipc` with `kernel-ipc` as well. Buffer sizes come
// from `nexus_abi::IPC_PAYLOAD_MAX` now, never from a local guess.
#[cfg(all(nexus_env = "os", feature = "os-lite", feature = "kernel-ipc"))]
mod os_kernel;
#[cfg(all(nexus_env = "os", feature = "os-lite", feature = "kernel-ipc"))]
pub use os_kernel::{
    set_default_target, supports_service_routing, KernelClient, KernelServer, PendingReply,
    ReplyCap,
};
