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
use crate::types::{VirtAddr, PageLen, SlotIndex};

// Typed decoders for seL4-style Decode→Check→Execute

#[derive(Copy, Clone)]
struct SpawnArgsTyped {
    entry_pc: VirtAddr,
    stack_sp: Option<VirtAddr>,
    as_handle: Option<AsHandle>,
    bootstrap_slot: SlotIndex,
}

impl SpawnArgsTyped {
    #[inline]
    fn decode(args: &Args) -> Result<Self, Error> {
        let entry_pc = VirtAddr::instr_aligned(args.get(0))
            .ok_or(AddressSpaceError::InvalidArgs)?;
        let stack_raw = args.get(1);
        let stack_sp = if stack_raw == 0 {
            None
        } else {
            Some(VirtAddr::page_aligned(stack_raw).ok_or(AddressSpaceError::InvalidArgs)?)
        };
        let raw_handle = args.get(2) as u32;
        let as_handle = AsHandle::from_raw(raw_handle);
        let bootstrap_slot = SlotIndex::decode(args.get(3));
        Ok(Self { entry_pc, stack_sp, as_handle, bootstrap_slot })
    }

    #[inline]
    fn check(&self) -> Result<(), Error> {
        if self.as_handle.is_some() && self.stack_sp.is_none() {
            return Err(AddressSpaceError::InvalidArgs.into());
        }
        if self.as_handle.is_none() && self.stack_sp.is_some() {
            return Err(AddressSpaceError::InvalidArgs.into());
        }
        Ok(())
    }
}

#[derive(Copy, Clone)]
struct AsMapArgsTyped {
    handle: AsHandle,
    vmo_slot: SlotIndex,
    va: VirtAddr,
    len: PageLen,
    prot: u32,
    flags: u32,
}

impl AsMapArgsTyped {
    #[inline]
    fn decode(args: &Args) -> Result<Self, Error> {
        let handle = AsHandle::from_raw(args.get(0) as u32)
            .ok_or(AddressSpaceError::InvalidHandle)?;
        let vmo_slot = SlotIndex::decode(args.get(1));
        let va = VirtAddr::page_aligned(args.get(2))
            .ok_or(AddressSpaceError::InvalidArgs)?;
        let len = PageLen::from_bytes_aligned(args.get(3) as u64)
            .ok_or(AddressSpaceError::InvalidArgs)?;
        let prot = args.get(4) as u32;
        let flags = args.get(5) as u32;
        Ok(Self { handle, vmo_slot, va, len, prot, flags })
    }

    #[inline]
    fn check(&self) -> Result<(), Error> {
        if self.len.raw() == 0 {
            return Err(AddressSpaceError::InvalidArgs.into());
        }
        // W^X
        if (self.prot & PROT_WRITE != 0) && (self.prot & PROT_EXEC != 0) {
            return Err(AddressSpaceError::from(MapError::PermissionDenied).into());
        }
        // Range check: ensure va + len fits
        self.va
            .checked_add(self.len.raw())
            .ok_or(AddressSpaceError::InvalidArgs)?;
        Ok(())
    }
}

#[derive(Copy, Clone)]
struct SendArgsTyped { slot: SlotIndex, ty: u16, flags: u16, len: u32 }

impl SendArgsTyped {
    #[inline]
    fn decode(args: &Args) -> Result<Self, Error> {
        Ok(Self {
            slot: SlotIndex::decode(args.get(0)),
            ty: args.get(1) as u16,
            flags: args.get(2) as u16,
            len: args.get(3) as u32,
        })
    }
    #[inline]
    fn check(&self) -> Result<(), Error> {
        // Keep len unconstrained for now; stage-policy minimal checks
        let _ = self.len;
        Ok(())
    }
}

#[derive(Copy, Clone)]
struct RecvArgsTyped { slot: SlotIndex }

impl RecvArgsTyped {
    #[inline]
    fn decode(args: &Args) -> Result<Self, Error> {
        Ok(Self { slot: SlotIndex::decode(args.get(0)) })
    }
    #[inline]
    fn check(&self) -> Result<(), Error> { Ok(()) }
}

#[derive(Copy, Clone)]
struct MapArgsTyped { slot: SlotIndex, va: VirtAddr, offset: usize, flags: PageFlags }

impl MapArgsTyped {
    #[inline]
    fn decode(args: &Args) -> Result<Self, Error> {
        Ok(Self {
            slot: SlotIndex::decode(args.get(0)),
            va: VirtAddr::page_aligned(args.get(1)).ok_or(AddressSpaceError::InvalidArgs)?,
            offset: args.get(2),
            flags: PageFlags::from_bits(args.get(3)).ok_or(AddressSpaceError::InvalidArgs)?,
        })
    }
    #[inline]
    fn check(&self) -> Result<(), Error> {
        if self.flags.contains(PageFlags::WRITE) && self.flags.contains(PageFlags::EXECUTE) {
            return Err(AddressSpaceError::from(MapError::PermissionDenied).into());
        }
        Ok(())
    }
}

#[derive(Copy, Clone)]
struct VmoCreateArgsTyped { slot_raw: usize, len: usize }

impl VmoCreateArgsTyped {
    #[inline]
    fn decode(args: &Args) -> Result<Self, Error> {
        Ok(Self { slot_raw: args.get(0), len: args.get(1) })
    }
    #[inline]
    fn check(&self) -> Result<(), Error> {
        if self.len == 0 { return Err(Error::Capability(CapError::PermissionDenied)); }
        Ok(())
    }
}

#[derive(Copy, Clone)]
struct VmoWriteArgsTyped { slot: SlotIndex, offset: usize, len: usize }

impl VmoWriteArgsTyped {
    #[inline]
    fn decode(args: &Args) -> Result<Self, Error> {
        Ok(Self { slot: SlotIndex::decode(args.get(0)), offset: args.get(1), len: args.get(2) })
    }
    #[inline]
    fn check(&self) -> Result<(), Error> { Ok(()) }
}

#[derive(Copy, Clone)]
struct CapTransferArgsTyped { child: task::Pid, parent_slot: SlotIndex, rights_bits: u32 }

impl CapTransferArgsTyped {
    #[inline]
    fn decode(args: &Args) -> Result<Self, Error> {
        Ok(Self {
            child: args.get(0) as task::Pid,
            parent_slot: SlotIndex::decode(args.get(1)),
            rights_bits: args.get(2) as u32,
        })
    }
    #[inline]
    fn check(&self) -> Result<Rights, Error> {
        Rights::from_bits(self.rights_bits).ok_or_else(|| {
            Error::Transfer(task::TransferError::Capability(CapError::PermissionDenied))
        })
    }
}

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
    crate::liveness::bump();
    ctx.scheduler.yield_current();
    if let Some(next) = ctx.scheduler.schedule_next() {
        ctx.tasks.set_current(next as task::Pid);
        if let Some(task) = ctx.tasks.task(next as task::Pid) {
            #[cfg(feature = "debug_uart")]
            {
                use core::fmt::Write as _;
                let mut w = crate::uart::raw_writer();
                let _ = write!(w, "YIELD-I: next pid={} sepc=0x{:x}\n", next, task.frame().sepc);
            }
            #[cfg(not(feature = "selftest_no_satp"))]
            {
                if let Some(handle) = task.address_space() {
                    ctx.address_spaces.activate(handle)?;
                }
            }
            #[cfg(feature = "selftest_no_satp")]
            let _ = task; // silence unused when SATP is disabled for selftests
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
    let typed = SendArgsTyped::decode(args)?; typed.check()?;
    let cap = ctx.tasks.current_caps_mut().derive(typed.slot.0, Rights::SEND)?;
    let endpoint = match cap.kind {
        CapabilityKind::Endpoint(id) => id,
        _ => return Err(Error::Capability(CapError::PermissionDenied)),
    };
    let header = MessageHeader::new(typed.slot.0 as u32, endpoint, typed.ty, typed.flags, typed.len);
    let payload = Vec::new();
    ctx.router.send(endpoint, ipc::Message::new(header, payload))?;
    Ok(typed.len as usize)
}

fn sys_recv(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let typed = RecvArgsTyped::decode(args)?; typed.check()?;
    let cap = ctx.tasks.current_caps_mut().derive(typed.slot.0, Rights::RECV)?;
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
    let typed = MapArgsTyped::decode(args)?; typed.check()?;
    let cap = ctx.tasks.current_caps_mut().derive(typed.slot.0, Rights::MAP)?;
    match cap.kind {
        CapabilityKind::Vmo { base, len } => {
            if typed.offset >= len {
                return Err(Error::Capability(CapError::PermissionDenied));
            }
            let va = typed.va;
            let pa = base + (typed.offset & !0xfff);
            let handle = ctx
                .tasks
                .current_task()
                .address_space()
                .ok_or(AddressSpaceError::InvalidHandle)?;
            ctx.address_spaces.map_page(handle, va.raw(), pa, typed.flags)?;
            Ok(0)
        }
        _ => Err(Error::Capability(CapError::PermissionDenied)),
    }
}

fn sys_vmo_create(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let typed = VmoCreateArgsTyped::decode(args)?; typed.check()?;
    // In this minimal path we grant MAP rights over a freshly allocated region.
    // The physical base is derived from a simple bump allocator seeded from a
    // bootstrap identity VMO. For now, use the existing slot 1 as a template.
    let template = ctx.tasks.current_caps_mut().get(1)?;
    let (base, avail) = match template.kind {
        CapabilityKind::Vmo { base, len } => (base, len),
        _ => return Err(Error::Capability(CapError::PermissionDenied)),
    };
    if typed.len == 0 || typed.len > avail {
        return Err(Error::Capability(CapError::PermissionDenied));
    }
    let aligned_len = (typed.len + 0xfff) & !0xfff;
    // Carve a subrange beginning at the template base. In a real kernel this
    // would maintain a free list; here we return the template for simplicity.
    let cap =
        Capability { kind: CapabilityKind::Vmo { base, len: aligned_len }, rights: Rights::MAP };
    let target = if typed.slot_raw == usize::MAX {
        ctx.tasks.current_caps_mut().allocate(cap)?
    } else {
        ctx.tasks.current_caps_mut().set(typed.slot_raw, cap)?;
        typed.slot_raw
    };
    Ok(target)
}

fn sys_vmo_write(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let typed = VmoWriteArgsTyped::decode(args)?; typed.check()?;
    // This is a stub: without real memory backing, acknowledge the write when
    // within range of the VMO capability.
    let cap = ctx.tasks.current_caps_mut().derive(typed.slot.0, Rights::MAP)?;
    match cap.kind {
        CapabilityKind::Vmo { base: _, len: vmo_len } => {
            if typed.offset + typed.len > vmo_len {
                return Err(Error::Capability(CapError::PermissionDenied));
            }
            Ok(typed.len)
        }
        _ => Err(Error::Capability(CapError::PermissionDenied)),
    }
}

fn sys_spawn(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let typed = SpawnArgsTyped::decode(args)?;
    typed.check()?;

    let parent = ctx.tasks.current_pid();
    let pid = ctx.tasks.spawn(
        parent,
        typed.entry_pc,
        typed.stack_sp,
        typed.as_handle,
        typed.bootstrap_slot,
        ctx.scheduler,
        ctx.router,
        ctx.address_spaces,
    )?;

    Ok(pid as usize)
}

fn sys_cap_transfer(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let typed = CapTransferArgsTyped::decode(args)?; let rights = typed.check()?;
    let parent = ctx.tasks.current_pid();
    let slot = ctx.tasks.transfer_cap(parent, typed.child, typed.parent_slot.0, rights)?;
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
    let typed = AsMapArgsTyped::decode(args)?;
    typed.check()?; // Check phase

    let cap = ctx.tasks.current_caps_mut().derive(typed.vmo_slot.0, Rights::MAP)?;
    let (base, vmo_len) = match cap.kind {
        CapabilityKind::Vmo { base, len } => (base, len as u64),
        _ => return Err(Error::Capability(CapError::PermissionDenied)),
    };

    let map_bytes = cmp::min(typed.len.raw() as u64, vmo_len);
    let aligned_bytes = map_bytes - (map_bytes % PAGE_SIZE as u64);
    if aligned_bytes == 0 {
        return Err(AddressSpaceError::InvalidArgs.into());
    }
    let pages = (aligned_bytes / PAGE_SIZE as u64) as usize;
    let span_bytes = pages
        .checked_mul(PAGE_SIZE)
        .ok_or(AddressSpaceError::InvalidArgs)?;
    typed.va.checked_add(span_bytes).ok_or(AddressSpaceError::InvalidArgs)?;

    let mut flags = PageFlags::VALID;
    if typed.prot & PROT_READ != 0 {
        flags |= PageFlags::READ;
    }
    if typed.prot & PROT_WRITE != 0 {
        flags |= PageFlags::WRITE;
    }
    if typed.prot & PROT_EXEC != 0 {
        flags |= PageFlags::EXECUTE;
    }
    if typed.flags & MAP_FLAG_USER != 0 {
        flags |= PageFlags::USER;
    }

    for page in 0..pages {
        let page_va = typed.va.raw()
            .checked_add(page * PAGE_SIZE)
            .ok_or(AddressSpaceError::InvalidArgs)?;
        let page_pa = base
            .checked_add(page * PAGE_SIZE)
            .ok_or(AddressSpaceError::InvalidArgs)?;
        ctx.address_spaces.map_page(typed.handle, page_va, page_pa, flags)?;
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
            .dispatch(SYSCALL_SPAWN, &mut ctx, &Args::new([0x1000, 0, 0, 0, 0, 0]))
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
