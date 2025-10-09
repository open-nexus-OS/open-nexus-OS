// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Syscall dispatcher and error handling.

pub mod api;

use core::fmt;

use crate::{cap, ipc};

/// Maximum number of syscalls supported by this increment.
const MAX_SYSCALL: usize = 16;

/// Result type used by syscall handlers.
pub type SysResult<T> = Result<T, Error>;

/// Syscall arguments passed in registers a0-a5.
#[derive(Default, Clone, Copy)]
pub struct Args {
    regs: [usize; 6],
}

impl Args {
    /// Creates a new argument pack from the provided registers.
    pub const fn new(regs: [usize; 6]) -> Self {
        Self { regs }
    }

    /// Returns the raw register at `index`.
    pub fn get(&self, index: usize) -> usize {
        self.regs[index]
    }
}

/// Public syscall numbers.
pub const SYSCALL_YIELD: usize = 0;
pub const SYSCALL_NSEC: usize = 1;
pub const SYSCALL_SEND: usize = 2;
pub const SYSCALL_RECV: usize = 3;
pub const SYSCALL_MAP: usize = 4;
pub const SYSCALL_VMO_CREATE: usize = 5;
pub const SYSCALL_VMO_WRITE: usize = 6;

/// Error returned by the dispatcher and handler stack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// Syscall number not present in the dispatch table.
    InvalidSyscall,
    /// Capability lookup failed.
    Capability(cap::CapError),
    /// IPC routing failed.
    Ipc(ipc::IpcError),
}

impl From<cap::CapError> for Error {
    fn from(value: cap::CapError) -> Self {
        Self::Capability(value)
    }
}

impl From<ipc::IpcError> for Error {
    fn from(value: ipc::IpcError) -> Self {
        Self::Ipc(value)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

/// Type alias for a syscall handler.
pub type Handler = fn(&mut api::Context<'_>, &Args) -> SysResult<usize>;

/// Dispatch table storing handlers by syscall number.
pub struct SyscallTable {
    handlers: [Option<Handler>; MAX_SYSCALL],
}

impl SyscallTable {
    /// Creates an empty dispatch table.
    pub const fn new() -> Self {
        const NONE: Option<Handler> = None;
        Self { handlers: [NONE; MAX_SYSCALL] }
    }

    /// Registers a handler.
    pub fn register(&mut self, number: usize, handler: Handler) {
        if number < MAX_SYSCALL {
            self.handlers[number] = Some(handler);
        }
    }

    /// Executes the handler referenced by `number`.
    pub fn dispatch(
        &self,
        number: usize,
        ctx: &mut api::Context<'_>,
        args: &Args,
    ) -> SysResult<usize> {
        self.handlers
            .get(number)
            .and_then(|entry| *entry)
            .ok_or(Error::InvalidSyscall)
            .and_then(|handler| handler(ctx, args))
    }
}
