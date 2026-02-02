// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Syscall dispatcher and error handling
//! OWNERS: @kernel-team
//! PUBLIC API: SyscallTable, Args, Error, Handler, SYSCALL_* IDs
//! DEPENDS_ON: cap, ipc, mm, task, syscall::api
//! INVARIANTS: Fixed MAX_SYSCALL window; stable IDs; decode/check/execute discipline
//! ADR: docs/adr/0001-runtime-roles-and-boundaries.md

pub mod api;

use core::fmt;

use crate::{cap, ipc, mm, task};

/// Maximum number of syscalls supported by this increment.
const MAX_SYSCALL: usize = 32;

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
pub const SYSCALL_SPAWN: usize = 7;
pub const SYSCALL_CAP_TRANSFER: usize = 8;
pub const SYSCALL_AS_CREATE: usize = 9;
pub const SYSCALL_AS_MAP: usize = 10;
pub const SYSCALL_EXIT: usize = 11;
pub const SYSCALL_WAIT: usize = 12;
pub const SYSCALL_EXEC: usize = 13;
/// IPC v1 (payload copy-in): see RFC-0005.
pub const SYSCALL_IPC_SEND_V1: usize = 14;
/// Exec loader v2: adds explicit service metadata (name ptr/len) for RFC-0004 provenance.
pub const SYSCALL_EXEC_V2: usize = 17;
/// Debug UART putc for userspace (best-effort, no permissions required).
pub const SYSCALL_DEBUG_PUTC: usize = 16;
/// IPC v1 (payload copy-out): see RFC-0005.
pub const SYSCALL_IPC_RECV_V1: usize = 18;
/// Create a new kernel IPC endpoint and return a capability slot for it (privileged; RFC-0005).
pub const SYSCALL_IPC_ENDPOINT_CREATE: usize = 19;
/// Drops a capability slot; if the capability has `Rights::MANAGE` and is an endpoint, closes it.
pub const SYSCALL_CAP_CLOSE: usize = 20;
/// Closes a kernel IPC endpoint referenced by a capability slot with `Rights::MANAGE`.
pub const SYSCALL_IPC_ENDPOINT_CLOSE: usize = 21;
/// Creates a new IPC endpoint using an explicit endpoint-factory capability (Phase-2 hardening).
pub const SYSCALL_IPC_ENDPOINT_CREATE_V2: usize = 22;
/// Creates a new IPC endpoint on behalf of `owner_pid` (init-lite bootstrap helper).
pub const SYSCALL_IPC_ENDPOINT_CREATE_FOR: usize = 23;
/// Clones a capability slot locally (creates a second reference in the caller).
pub const SYSCALL_CAP_CLONE: usize = 24;
/// Returns the current task PID.
pub const SYSCALL_GETPID: usize = 25;
/// Receives an IPC message and additionally returns sender service identity metadata (v2).
pub const SYSCALL_IPC_RECV_V2: usize = 26;
/// Maps a device MMIO capability window into the caller's address space (USER|RW, never EXEC).
///
/// This is the kernel primitive required for userspace virtio drivers on QEMU `virt`.
pub const SYSCALL_MMIO_MAP: usize = 27;
/// Queries a capability slot and writes (kind_tag, base, len) to a user buffer.
///
/// This is a small introspection primitive needed by userspace drivers to obtain physical
/// addresses for DMA-capable resources (e.g., VMOs) without exposing ambient physical memory.
pub const SYSCALL_CAP_QUERY: usize = 28;
/// Creates a DeviceMmio capability in the caller's cap table (privileged; init-only).
pub const SYSCALL_DEVICE_CAP_CREATE: usize = 30;
/// Transfers a capability into a specific slot in the child task.
pub const SYSCALL_CAP_TRANSFER_TO: usize = 31;
/// Returns the last spawn failure reason for the current task (RFC-0013).
pub const SYSCALL_SPAWN_LAST_ERROR: usize = 29;

/// Error returned by the dispatcher and handler stack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// Syscall number not present in the dispatch table.
    InvalidSyscall,
    /// Capability lookup failed.
    Capability(cap::CapError),
    /// IPC routing failed.
    Ipc(ipc::IpcError),
    /// Spawn operation failed.
    Spawn(task::SpawnError),
    /// Capability transfer failed.
    Transfer(task::TransferError),
    /// Address-space manager reported an error.
    AddressSpace(mm::AddressSpaceError),
    /// Process lifecycle operation failed.
    Wait(task::WaitError),
    /// Current task terminated and should not resume.
    TaskExit,
    /// Request an immediate reschedule **without** advancing the caller PC (`sepc`).
    ///
    /// Rationale: syscall handlers must not "switch tasks" by calling `sys_yield()` internally,
    /// because that would mutate `current_pid` without going through the trap-exit path that
    /// also switches SATP (address space). Doing so can accidentally run userspace code in the
    /// wrong address space. Instead, handlers return `Reschedule` and the kernel performs the
    /// switch on trap exit; the same syscall is retried when the task runs again.
    Reschedule,
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

impl From<task::SpawnError> for Error {
    fn from(value: task::SpawnError) -> Self {
        Self::Spawn(value)
    }
}

impl From<task::TransferError> for Error {
    fn from(value: task::TransferError) -> Self {
        Self::Transfer(value)
    }
}

impl From<mm::AddressSpaceError> for Error {
    fn from(value: mm::AddressSpaceError) -> Self {
        Self::AddressSpace(value)
    }
}

impl From<task::WaitError> for Error {
    fn from(value: task::WaitError) -> Self {
        Self::Wait(value)
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
    #[must_use]
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

    #[inline]
    #[allow(dead_code)]
    pub fn debug_handler_addr(&self, number: usize) -> Option<usize> {
        self.handlers.get(number).and_then(|entry| (*entry).map(|f| f as usize))
    }
}
