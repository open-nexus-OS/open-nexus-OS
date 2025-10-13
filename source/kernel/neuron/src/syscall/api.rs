// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Syscall handlers exposed to the dispatcher.

extern crate alloc;

use alloc::vec::Vec;
use core::cmp;

use crate::{
    cap::{CapError, Capability, CapabilityKind, Rights},
    hal::Timer,
    ipc::{self, header::MessageHeader},
    mm::{AddressSpaceError, AddressSpaceManager, AsHandle, MapError, PageFlags, PAGE_SIZE},
    sched::Scheduler,
    task,
};

use super::{
    Args, Error, SysResult, SyscallTable, SYSCALL_AS_CREATE, SYSCALL_AS_MAP, SYSCALL_CAP_TRANSFER,
    SYSCALL_MAP, SYSCALL_NSEC, SYSCALL_RECV, SYSCALL_SEND, SYSCALL_SPAWN, SYSCALL_VMO_CREATE,
    SYSCALL_VMO_WRITE, SYSCALL_YIELD,
};

/// Execution context shared across syscalls.
pub struct Context<'a> {
    pub scheduler: &'a mut Scheduler,
    pub tasks: &'a mut task::TaskTable,
    pub router: &'a mut ipc::Router,
    pub address_spaces: &'a mut AddressSpaceManager,
    pub timer: &'a dyn Timer,
    pub last_message: Option<ipc::Message>,
}

impl<'a> Context<'a> {
    /// Creates a new context for the current task.
    pub fn new(
        scheduler: &'a mut Scheduler,
        tasks: &'a mut task::TaskTable,
        router: &'a mut ipc::Router,
        address_spaces: &'a mut AddressSpaceManager,
        timer: &'a dyn Timer,
    ) -> Self {
        Self { scheduler, tasks, router, address_spaces, timer, last_message: None }
    }

    /// Returns the last received message header for inspection.
    #[cfg(test)]
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
    table.register(SYSCALL_VMO_CREATE, sys_vmo_create);
    table.register(SYSCALL_VMO_WRITE, sys_vmo_write);
    table.register(SYSCALL_SPAWN, sys_spawn);
    table.register(SYSCALL_CAP_TRANSFER, sys_cap_transfer);
    table.register(SYSCALL_AS_CREATE, sys_as_create);
    table.register(SYSCALL_AS_MAP, sys_as_map);
}

fn sys_yield(ctx: &mut Context<'_>, _args: &Args) -> SysResult<usize> {
    ctx.scheduler.yield_current();
    if let Some(next) = ctx.scheduler.schedule_next() {
        ctx.tasks.set_current(next as task::Pid);
        if let Some(task) = ctx.tasks.task(next as task::Pid) {
            if let Some(handle) = task.address_space() {
                ctx.address_spaces.activate(handle)?;
            }
        }
        Ok(next as usize)
    } else {
        Ok(ctx.tasks.current_pid() as usize)
    }
}

fn sys_nsec(ctx: &mut Context<'_>, _args: &Args) -> SysResult<usize> {
    Ok(ctx.timer.now() as usize)
}

fn sys_send(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let slot = args.get(0);
    let ty = args.get(1) as u16;
    let flags = args.get(2) as u16;
    let len = args.get(3) as u32;
    let cap = ctx.tasks.current_caps_mut().derive(slot, Rights::SEND)?;
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
    let cap = ctx.tasks.current_caps_mut().derive(slot, Rights::RECV)?;
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
    let offset = args.get(2);
    let flags = PageFlags::from_bits_truncate(args.get(3));
    let cap = ctx.tasks.current_caps_mut().derive(slot, Rights::MAP)?;
    match cap.kind {
        CapabilityKind::Vmo { base, len } => {
            if offset >= len {
                return Err(Error::Capability(CapError::PermissionDenied));
            }
            let pa = base + (offset & !0xfff);
            let handle = ctx
                .tasks
                .current_task()
                .address_space()
                .ok_or(AddressSpaceError::InvalidHandle)?;
            ctx.address_spaces.map_page(handle, va, pa, flags)?;
            Ok(0)
        }
        _ => Err(Error::Capability(CapError::PermissionDenied)),
    }
}

fn sys_vmo_create(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let slot = args.get(0);
    let len = args.get(1);
    // In this minimal path we grant MAP rights over a freshly allocated region.
    // The physical base is derived from a simple bump allocator seeded from a
    // bootstrap identity VMO. For now, use the existing slot 1 as a template.
    let template = ctx.tasks.current_caps_mut().get(1)?;
    let (base, avail) = match template.kind {
        CapabilityKind::Vmo { base, len } => (base, len),
        _ => return Err(Error::Capability(CapError::PermissionDenied)),
    };
    if len == 0 || len > avail {
        return Err(Error::Capability(CapError::PermissionDenied));
    }
    let aligned_len = (len + 0xfff) & !0xfff;
    // Carve a subrange beginning at the template base. In a real kernel this
    // would maintain a free list; here we return the template for simplicity.
    let cap =
        Capability { kind: CapabilityKind::Vmo { base, len: aligned_len }, rights: Rights::MAP };
    let target = if slot == usize::MAX {
        ctx.tasks.current_caps_mut().allocate(cap)?
    } else {
        ctx.tasks.current_caps_mut().set(slot, cap)?;
        slot
    };
    Ok(target)
}

fn sys_vmo_write(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let slot = args.get(0);
    let offset = args.get(1);
    let len = args.get(2);
    // This is a stub: without real memory backing, acknowledge the write when
    // within range of the VMO capability.
    let cap = ctx.tasks.current_caps_mut().derive(slot, Rights::MAP)?;
    match cap.kind {
        CapabilityKind::Vmo { base: _, len: vmo_len } => {
            if offset + len > vmo_len {
                return Err(Error::Capability(CapError::PermissionDenied));
            }
            Ok(len)
        }
        _ => Err(Error::Capability(CapError::PermissionDenied)),
    }
}

fn sys_spawn(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let entry_pc = args.get(0) as u64;
    let stack_sp = args.get(1) as u64;
    let raw_handle = args.get(2) as u32;
    let address_space = AsHandle::from_raw(raw_handle);
    let bootstrap_slot = args.get(3) as u32;
    let parent = ctx.tasks.current_pid();
    let pid = ctx.tasks.spawn(
        parent,
        entry_pc,
        stack_sp,
        address_space,
        bootstrap_slot,
        ctx.scheduler,
        ctx.router,
        ctx.address_spaces,
    )?;
    Ok(pid as usize)
}

fn sys_cap_transfer(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let child = args.get(0) as task::Pid;
    let parent_slot = args.get(1);
    let rights_bits = args.get(2) as u32;
    let rights = Rights::from_bits(rights_bits).ok_or_else(|| {
        Error::Transfer(task::TransferError::Capability(CapError::PermissionDenied))
    })?;
    let parent = ctx.tasks.current_pid();
    let slot = ctx.tasks.transfer_cap(parent, child, parent_slot, rights)?;
    Ok(slot)
}

const PROT_READ: u32 = 1 << 0;
const PROT_WRITE: u32 = 1 << 1;
const PROT_EXEC: u32 = 1 << 2;

const MAP_FLAG_USER: u32 = 1 << 0;

fn sys_as_create(ctx: &mut Context<'_>, _args: &Args) -> SysResult<usize> {
    let handle = ctx.address_spaces.create()?;
    Ok(handle.to_raw() as usize)
}

fn sys_as_map(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let raw_handle = args.get(0) as u32;
    let vmo_slot = args.get(1);
    let va = args.get(2);
    let len = args.get(3) as u64;
    let prot = args.get(4) as u32;
    let map_flags = args.get(5) as u32;

    let handle = AsHandle::from_raw(raw_handle).ok_or(AddressSpaceError::InvalidHandle)?;
    if len == 0 || va % PAGE_SIZE != 0 || len % PAGE_SIZE as u64 != 0 {
        return Err(AddressSpaceError::InvalidArgs.into());
    }
    if prot & PROT_WRITE != 0 && prot & PROT_EXEC != 0 {
        return Err(AddressSpaceError::from(MapError::PermissionDenied).into());
    }

    let cap = ctx.tasks.current_caps_mut().derive(vmo_slot, Rights::MAP)?;
    let (base, vmo_len) = match cap.kind {
        CapabilityKind::Vmo { base, len } => (base, len as u64),
        _ => return Err(Error::Capability(CapError::PermissionDenied)),
    };

    let map_bytes = cmp::min(len, vmo_len);
    let aligned_bytes = map_bytes - (map_bytes % PAGE_SIZE as u64);
    if aligned_bytes == 0 {
        return Err(AddressSpaceError::InvalidArgs.into());
    }
    let pages = (aligned_bytes / PAGE_SIZE as u64) as usize;
    let span_bytes = pages
        .checked_mul(PAGE_SIZE)
        .ok_or(AddressSpaceError::InvalidArgs)?;
    va.checked_add(span_bytes).ok_or(AddressSpaceError::InvalidArgs)?;

    let mut flags = PageFlags::VALID;
    if prot & PROT_READ != 0 {
        flags |= PageFlags::READ;
    }
    if prot & PROT_WRITE != 0 {
        flags |= PageFlags::WRITE;
    }
    if prot & PROT_EXEC != 0 {
        flags |= PageFlags::EXECUTE;
    }
    if map_flags & MAP_FLAG_USER != 0 {
        flags |= PageFlags::USER;
    }

    for page in 0..pages {
        let page_va = va
            .checked_add(page * PAGE_SIZE)
            .ok_or(AddressSpaceError::InvalidArgs)?;
        let page_pa = base
            .checked_add(page * PAGE_SIZE)
            .ok_or(AddressSpaceError::InvalidArgs)?;
        ctx.address_spaces.map_page(handle, page_va, page_pa, flags)?;
    }

    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        cap::{Capability, CapabilityKind, Rights},
        mm::AddressSpaceManager,
        syscall::{
            Args, SyscallTable, SYSCALL_CAP_TRANSFER, SYSCALL_RECV, SYSCALL_SEND, SYSCALL_SPAWN,
        },
        task::TaskTable,
        BootstrapMsg,
    };

    #[test]
    fn send_recv_roundtrip() {
        let mut scheduler = Scheduler::new();
        let mut tasks = TaskTable::new();
        {
            let caps = tasks.bootstrap_mut().caps_mut();
            let _ = caps.set(
                0,
                Capability {
                    kind: CapabilityKind::Endpoint(0),
                    rights: Rights::SEND | Rights::RECV,
                },
            );
        }
        let mut router = ipc::Router::new(1);
        let mut as_manager = AddressSpaceManager::new();
        let kernel_as = as_manager.create().unwrap();
        as_manager.attach(kernel_as, 0).unwrap();
        tasks.bootstrap_mut().address_space = Some(kernel_as);
        let timer = crate::hal::virt::VirtMachine::new();
        let mut ctx = Context::new(
            &mut scheduler,
            &mut tasks,
            &mut router,
            &mut as_manager,
            timer.timer(),
        );
        let mut table = SyscallTable::new();
        install_handlers(&mut table);

        table.dispatch(SYSCALL_SEND, &mut ctx, &Args::new([0, 1, 0, 0, 0, 0])).unwrap();
        let len = table.dispatch(SYSCALL_RECV, &mut ctx, &Args::new([0, 0, 0, 0, 0, 0])).unwrap();
        assert_eq!(len, 0);
        assert!(ctx.last_message().is_some());
    }

    #[test]
    fn spawn_and_transfer_syscalls() {
        let mut scheduler = Scheduler::new();
        let mut tasks = TaskTable::new();
        {
            let caps = tasks.bootstrap_mut().caps_mut();
            caps.set(
                0,
                Capability {
                    kind: CapabilityKind::Endpoint(0),
                    rights: Rights::SEND | Rights::RECV,
                },
            )
            .unwrap();
        }
        let mut router = ipc::Router::new(2);
        let mut as_manager = AddressSpaceManager::new();
        let kernel_as = as_manager.create().unwrap();
        as_manager.attach(kernel_as, 0).unwrap();
        tasks.bootstrap_mut().address_space = Some(kernel_as);
        let timer = crate::hal::virt::VirtMachine::new();
        let mut ctx = Context::new(
            &mut scheduler,
            &mut tasks,
            &mut router,
            &mut as_manager,
            timer.timer(),
        );
        let mut table = SyscallTable::new();
        install_handlers(&mut table);

        let child = table
            .dispatch(SYSCALL_SPAWN, &mut ctx, &Args::new([0x1000, 0x2000, 0, 0, 0, 0]))
            .unwrap() as u32;
        assert_eq!(child, 1);
        let msg = ctx.router.recv(0).unwrap();
        assert_eq!(msg.payload.len(), core::mem::size_of::<BootstrapMsg>());

        let slot = table
            .dispatch(
                SYSCALL_CAP_TRANSFER,
                &mut ctx,
                &Args::new([child as usize, 0, Rights::SEND.bits() as usize, 0, 0, 0]),
            )
            .unwrap();
        assert_ne!(slot, 0);
        let cap = ctx.tasks.caps_of(child).unwrap().get(slot).unwrap();
        assert_eq!(cap.rights, Rights::SEND);
    }
}
