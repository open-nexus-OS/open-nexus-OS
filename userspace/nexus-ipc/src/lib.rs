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
    all(
        feature = "os-lite",
        nexus_env = "os",
        target_arch = "riscv64",
        target_os = "none"
    ),
    no_std
)]

#[cfg(all(nexus_env = "os", feature = "os-lite"))]
extern crate alloc;

use core::fmt;
use core::time::Duration;

#[cfg(all(nexus_env = "os", feature = "os-lite"))]
use alloc::vec::Vec;
#[cfg(not(all(nexus_env = "os", feature = "os-lite")))]
use std::vec::Vec;

/// Result type returned by IPC operations.
pub type Result<T> = core::result::Result<T, IpcError>;

/// Behaviour of a blocking call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wait {
    /// Block until the operation completes.
    Blocking,
    /// Return immediately if no progress can be made.
    NonBlocking,
    /// Block until either the operation completes or the timeout expires.
    Timeout(Duration),
}

impl Wait {
    /// Returns `true` when the caller requested a non-blocking attempt.
    pub const fn is_non_blocking(self) -> bool {
        matches!(self, Self::NonBlocking)
    }

    /// Converts a [`Wait::Timeout`] variant into its [`Duration`].
    pub const fn timeout(self) -> Option<Duration> {
        match self {
            Self::Timeout(duration) => Some(duration),
            Self::Blocking | Self::NonBlocking => None,
        }
    }
}

/// Errors produced by the IPC runtime.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IpcError {
    /// Operation could not progress without blocking.
    WouldBlock,
    /// The caller exceeded the requested timeout.
    Timeout,
    /// The opposite endpoint disconnected.
    Disconnected,
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
            Self::Kernel(err) => write!(f, "kernel rejected ipc request: {err:?}"),
            Self::Unsupported => write!(f, "ipc not supported for this configuration"),
        }
    }
}

#[cfg(nexus_env = "host")]
impl std::error::Error for IpcError {}

impl From<nexus_abi::IpcError> for IpcError {
    fn from(err: nexus_abi::IpcError) -> Self {
        Self::Kernel(err)
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

#[cfg(nexus_env = "host")]
mod host;
#[cfg(nexus_env = "host")]
pub use host::{loopback_channel, LoopbackClient, LoopbackServer};

#[cfg(all(nexus_env = "os", not(feature = "os-lite")))]
mod os;
#[cfg(all(nexus_env = "os", not(feature = "os-lite")))]
pub use os::{set_default_target, KernelClient, KernelServer};

// no_std OS-lite backend (OpenHarmony LiteIPC-like), enabled via feature flag
#[cfg(all(nexus_env = "os", feature = "os-lite"))]
mod os_lite;
#[cfg(all(nexus_env = "os", feature = "os-lite"))]
pub use os_lite::{set_default_target, LiteClient as KernelClient, LiteServer as KernelServer};
