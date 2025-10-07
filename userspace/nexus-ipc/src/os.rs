// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Kernel-backed IPC implementation.

use crate::{Client, IpcError, Result, Server, Wait};

/// Client backed by kernel IPC. The implementation is provided once the kernel
/// syscall layer is ready; for now all operations return [`IpcError::Unsupported`].
pub struct KernelClient;

impl KernelClient {
    /// Attempts to create a new kernel IPC client bound to the process' default
    /// channel. The kernel backend is not wired yet, therefore the function
    /// currently returns [`Err(IpcError::Unsupported)`].
    pub const fn new() -> Result<Self> {
        Err(IpcError::Unsupported)
    }
}

impl Client for KernelClient {
    fn send(&self, _frame: &[u8], _wait: Wait) -> Result<()> {
        Err(IpcError::Unsupported)
    }

    fn recv(&self, _wait: Wait) -> Result<Vec<u8>> {
        Err(IpcError::Unsupported)
    }
}

/// Server backed by kernel IPC.
pub struct KernelServer;

impl KernelServer {
    /// Attempts to create a new kernel IPC server bound to the process' default
    /// channel. The syscall wiring is still pending and this function returns
    /// [`Err(IpcError::Unsupported)`] until the kernel integration lands.
    pub const fn new() -> Result<Self> {
        Err(IpcError::Unsupported)
    }
}

impl Server for KernelServer {
    fn recv(&self, _wait: Wait) -> Result<Vec<u8>> {
        Err(IpcError::Unsupported)
    }

    fn send(&self, _frame: &[u8], _wait: Wait) -> Result<()> {
        Err(IpcError::Unsupported)
    }
}
