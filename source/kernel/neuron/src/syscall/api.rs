// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Syscall handlers exposed to the dispatcher.

extern crate alloc;

use alloc::vec::Vec;

use crate::{
    cap::{CapError, CapTable, CapabilityKind, Rights},
    hal::Timer,
    ipc::{self, header::MessageHeader},
    mm::{PageFlags, PageTable},
    sched::Scheduler,
};

use super::{Args, Error, SysResult, SyscallTable, SYSCALL_MAP, SYSCALL_NSEC, SYSCALL_RECV, SYSCALL_SEND, SYSCALL_YIELD};

/// Execution context shared across syscalls.
pub struct Context<'a> {
    pub scheduler: &'a mut Scheduler,
    pub caps: &'a mut CapTable,
    pub router: &'a mut ipc::Router,
    pub address_space: &'a mut PageTable,
    pub timer: &'a dyn Timer,
    pub last_message: Option<ipc::Message>,
}

impl<'a> Context<'a> {
    /// Creates a new context for the current task.
    pub fn new(
        scheduler: &'a mut Scheduler,
        caps: &'a mut CapTable,
        router: &'a mut ipc::Router,
        address_space: &'a mut PageTable,
        timer: &'a dyn Timer,
    ) -> Self {
        Self { scheduler, caps, router, address_space, timer, last_message: None }
    }

    /// Returns the last received message header for inspection.
    pub fn last_message(&self) -> Option<&ipc::Message> {
        self.last_message.as_ref()
    }
}

/// Registers the default set of syscall handlers.
pub fn install_handlers(table: &mut SyscallTable) {
    table.register(SYSCALL_YIELD, sys_yield);
    table.register(SYSCALL_NSEC, sys_nsec);
    table.register(SYSCALL_SEND, sys_send);
    table.register(SYSCALL_RECV, sys_recv);
    table.register(SYSCALL_MAP, sys_map);
}

fn sys_yield(ctx: &mut Context<'_>, _args: &Args) -> SysResult<usize> {
    Ok(ctx.scheduler.schedule_next().unwrap_or_default() as usize)
}

fn sys_nsec(ctx: &mut Context<'_>, _args: &Args) -> SysResult<usize> {
    Ok(ctx.timer.now() as usize)
}

fn sys_send(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let slot = args.get(0);
    let ty = args.get(1) as u16;
    let flags = args.get(2) as u16;
    let len = args.get(3) as u32;
    let cap = ctx.caps.derive(slot, Rights::SEND)?;
    let endpoint = match cap.kind {
        CapabilityKind::Endpoint(id) => id,
        _ => return Err(Error::Capability(CapError::PermissionDenied)),
    };
    let header = MessageHeader::new(slot as u32, endpoint, ty, flags, len);
    let payload = Vec::new();
    ctx.router.send(endpoint, ipc::Message::new(header, payload))?;
    Ok(len as usize)
}

fn sys_recv(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let slot = args.get(0);
    let cap = ctx.caps.derive(slot, Rights::RECV)?;
    let endpoint = match cap.kind {
        CapabilityKind::Endpoint(id) => id,
        _ => return Err(Error::Capability(CapError::PermissionDenied)),
    };
    let message = ctx.router.recv(endpoint)?;
    let len = message.header.len as usize;
    ctx.last_message = Some(message);
    Ok(len)
}

fn sys_map(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let slot = args.get(0);
    let va = args.get(1);
    let pa = args.get(2);
    let flags = PageFlags::from_bits_truncate(args.get(3));
    let cap = ctx.caps.derive(slot, Rights::MAP)?;
    match cap.kind {
        CapabilityKind::Vmo { base, len } => {
            if pa < base || pa >= base + len {
                return Err(Error::Capability(CapError::PermissionDenied));
            }
            ctx.address_space.map(va, pa, flags).map_err(|_| Error::Capability(CapError::PermissionDenied))?;
            Ok(0)
        }
        _ => Err(Error::Capability(CapError::PermissionDenied)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        cap::{Capability, CapabilityKind, Rights},
        syscall::{Args, SyscallTable, SYSCALL_RECV, SYSCALL_SEND},
    };

    #[test]
    fn send_recv_roundtrip() {
        let mut scheduler = Scheduler::new();
        let mut caps = CapTable::new();
        let _ = caps.set(0, Capability { kind: CapabilityKind::Endpoint(0), rights: Rights::SEND | Rights::RECV });
        let mut router = ipc::Router::new(1);
        let mut aspace = PageTable::new();
        let timer = crate::hal::virt::VirtMachine::new();
        let mut ctx = Context::new(&mut scheduler, &mut caps, &mut router, &mut aspace, timer.timer());
        let mut table = SyscallTable::new();
        install_handlers(&mut table);

        table.dispatch(SYSCALL_SEND, &mut ctx, &Args::new([0, 1, 0, 0, 0, 0])).unwrap();
        let len = table.dispatch(SYSCALL_RECV, &mut ctx, &Args::new([0, 0, 0, 0, 0, 0])).unwrap();
        assert_eq!(len, 0);
        assert!(ctx.last_message().is_some());
    }
}
