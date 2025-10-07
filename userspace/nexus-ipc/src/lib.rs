// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Minimal IPC runtime abstractions shared by host tests and the OS build.
//!
//! The host backend uses in-process channels to emulate kernel IPC and allows
//! unit tests to execute Cap'n Proto request/response cycles without booting
//! the full system. The OS backend delegates to the [`nexus-abi`] syscall
//! wrappers and will be wired to the kernel in subsequent commits.

#![forbid(unsafe_code)]
#![deny(clippy::all, missing_docs)]
#![allow(unexpected_cfgs)]

use core::time::Duration;

use thiserror::Error;

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
#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum IpcError {
    /// Operation could not progress without blocking.
    #[error("operation would block")]
    WouldBlock,
    /// The caller exceeded the requested timeout.
    #[error("operation timed out")]
    Timeout,
    /// The opposite endpoint disconnected.
    #[error("peer disconnected")]
    Disconnected,
    /// Kernel returned an IPC failure.
    #[error("kernel rejected ipc request: {0:?}")]
    Kernel(nexus_abi::IpcError),
    /// IPC is not available under the current build.
    #[error("ipc not supported for this configuration")]
    Unsupported,
}

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

#[cfg(nexus_env = "os")]
mod os;
#[cfg(nexus_env = "os")]
pub use os::{KernelClient, KernelServer};
