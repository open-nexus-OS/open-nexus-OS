// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Syscall handlers exposed to the dispatcher
//! OWNERS: @kernel-team
//! PUBLIC API: install_handlers(table), Context, Args, SysResult
//! DEPENDS_ON: sched::Scheduler, task::TaskTable, ipc::Router, mm::AddressSpaceManager
//! INVARIANTS: Stable syscall IDs; Decode→Check→Execute pattern; W^X for user mappings
//! ADR: docs/adr/0001-runtime-roles-and-boundaries.md

extern crate alloc;

use alloc::vec::Vec;
use core::cmp;
use core::ptr;

use crate::types::{PageLen, SlotIndex, VirtAddr};
use crate::{
    cap::{CapError, Capability, CapabilityKind, Rights},
    hal::Timer,
    ipc::{self, header::MessageHeader},
    mm::{
        AddressSpaceError, AddressSpaceManager, AsHandle, MapError, PageFlags, PAGE_SIZE,
        USER_VMO_ARENA_LEN,
    },
    sched::Scheduler,
    task,
};
use core::slice;
use spin::Mutex;

use crate::task::BlockReason;

// Typed decoders for seL4-style Decode→Check→Execute

#[derive(Copy, Clone)]
struct SpawnArgsTyped {
    entry_pc: VirtAddr,
    stack_sp: Option<VirtAddr>,
    as_handle: Option<AsHandle>,
    bootstrap_slot: SlotIndex,
    global_pointer: usize,
}

#[derive(Copy, Clone)]
struct ExecArgsTyped {
    elf_ptr: usize,
    elf_len: usize,
    stack_pages: usize,
    global_pointer: usize,
}

#[derive(Copy, Clone)]
struct ExecV2ArgsTyped {
    elf_ptr: usize,
    elf_len: usize,
    stack_pages: usize,
    global_pointer: usize,
    name_ptr: usize,
    name_len: usize,
}

impl ExecArgsTyped {
    #[inline]
    fn decode(args: &Args) -> Result<Self, Error> {
        let elf_ptr = args.get(0);
        let elf_len = args.get(1);
        let stack_pages = args.get(2);
        let global_pointer = args.get(3);
        if elf_len == 0 || stack_pages == 0 {
            return Err(AddressSpaceError::InvalidArgs.into());
        }
        Ok(Self { elf_ptr, elf_len, stack_pages, global_pointer })
    }

    #[inline]
    fn check(&self) -> Result<(), Error> {
        ensure_user_slice(self.elf_ptr, self.elf_len)?;
        if self.stack_pages == 0 {
            return Err(AddressSpaceError::InvalidArgs.into());
        }
        Ok(())
    }
}

impl ExecV2ArgsTyped {
    #[inline]
    fn decode(args: &Args) -> Result<Self, Error> {
        let elf_ptr = args.get(0);
        let elf_len = args.get(1);
        let stack_pages = args.get(2);
        let global_pointer = args.get(3);
        let name_ptr = args.get(4);
        let name_len = args.get(5);
        if elf_len == 0 || stack_pages == 0 {
            return Err(AddressSpaceError::InvalidArgs.into());
        }
        Ok(Self { elf_ptr, elf_len, stack_pages, global_pointer, name_ptr, name_len })
    }

    #[inline]
    fn check(&self) -> Result<(), Error> {
        ensure_user_slice(self.elf_ptr, self.elf_len)?;
        if self.name_len != 0 {
            const MAX_NAME_LEN: usize = 64;
            if self.name_len > MAX_NAME_LEN {
                return Err(AddressSpaceError::InvalidArgs.into());
            }
            ensure_user_slice(self.name_ptr, self.name_len)?;
        }
        Ok(())
    }
}

impl SpawnArgsTyped {
    #[inline]
    fn decode(args: &Args) -> Result<Self, Error> {
        let entry_pc =
            VirtAddr::instr_aligned(args.get(0)).ok_or(AddressSpaceError::InvalidArgs)?;
        let stack_raw = args.get(1);
        let stack_sp = if stack_raw == 0 {
            None
        } else {
            // Accept a normal stack pointer (not necessarily page aligned), but require a canonical VA.
            Some(VirtAddr::new(stack_raw).ok_or(AddressSpaceError::InvalidArgs)?)
        };
        let raw_handle = args.get(2) as u32;
        let as_handle = AsHandle::from_raw(raw_handle);
        let bootstrap_slot = SlotIndex::decode(args.get(3));
        let global_pointer = args.get(4);
        Ok(Self { entry_pc, stack_sp, as_handle, bootstrap_slot, global_pointer })
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
        let handle =
            AsHandle::from_raw(args.get(0) as u32).ok_or(AddressSpaceError::InvalidHandle)?;
        let vmo_slot = SlotIndex::decode(args.get(1));
        let va = VirtAddr::page_aligned(args.get(2)).ok_or(AddressSpaceError::InvalidArgs)?;
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
        self.va.checked_add(self.len.raw()).ok_or(AddressSpaceError::InvalidArgs)?;
        Ok(())
    }
}

#[derive(Copy, Clone)]
struct SendArgsTyped {
    slot: SlotIndex,
    ty: u16,
    flags: u16,
    len: u32,
}

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
struct RecvArgsTyped {
    slot: SlotIndex,
}

impl RecvArgsTyped {
    #[inline]
    fn decode(args: &Args) -> Result<Self, Error> {
        Ok(Self { slot: SlotIndex::decode(args.get(0)) })
    }
    #[inline]
    fn check(&self) -> Result<(), Error> {
        Ok(())
    }
}

const MAX_FRAME_BYTES: usize = 8 * 1024;
const IPC_SYS_NONBLOCK: usize = 1 << 0;
const IPC_SYS_TRUNCATE: usize = 1 << 1;

#[derive(Copy, Clone)]
struct IpcSendV1ArgsTyped {
    slot: SlotIndex,
    header_ptr: usize,
    payload_ptr: usize,
    payload_len: usize,
    sys_flags: usize,
    deadline_ns: u64,
}

impl IpcSendV1ArgsTyped {
    #[inline]
    fn decode(args: &Args) -> Result<Self, Error> {
        Ok(Self {
            slot: SlotIndex::decode(args.get(0)),
            header_ptr: args.get(1),
            payload_ptr: args.get(2),
            payload_len: args.get(3),
            sys_flags: args.get(4),
            deadline_ns: args.get(5) as u64,
        })
    }

    #[inline]
    fn check(&self) -> Result<(), Error> {
        ensure_user_slice(self.header_ptr, 16)?;
        if self.payload_len > MAX_FRAME_BYTES {
            return Err(AddressSpaceError::InvalidArgs.into());
        }
        if self.payload_len != 0 {
            ensure_user_slice(self.payload_ptr, self.payload_len)?;
        }
        Ok(())
    }
}

#[derive(Copy, Clone)]
struct IpcRecvV1ArgsTyped {
    slot: SlotIndex,
    header_out_ptr: usize,
    payload_out_ptr: usize,
    payload_out_max: usize,
    sys_flags: usize,
    deadline_ns: u64,
}

impl IpcRecvV1ArgsTyped {
    #[inline]
    fn decode(args: &Args) -> Result<Self, Error> {
        Ok(Self {
            slot: SlotIndex::decode(args.get(0)),
            header_out_ptr: args.get(1),
            payload_out_ptr: args.get(2),
            payload_out_max: args.get(3),
            sys_flags: args.get(4),
            deadline_ns: args.get(5) as u64,
        })
    }

    #[inline]
    fn check(&self) -> Result<(), Error> {
        ensure_user_slice(self.header_out_ptr, 16)?;
        if self.payload_out_max != 0 {
            ensure_user_slice(self.payload_out_ptr, self.payload_out_max)?;
        }
        Ok(())
    }
}

#[derive(Copy, Clone)]
struct MapArgsTyped {
    slot: SlotIndex,
    va: VirtAddr,
    offset: usize,
    flags: PageFlags,
}

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
struct MmioMapArgsTyped {
    slot: SlotIndex,
    va: VirtAddr,
    offset: usize,
}

impl MmioMapArgsTyped {
    #[inline]
    fn decode(args: &Args) -> Result<Self, Error> {
        Ok(Self {
            slot: SlotIndex::decode(args.get(0)),
            va: VirtAddr::page_aligned(args.get(1)).ok_or(AddressSpaceError::InvalidArgs)?,
            offset: args.get(2),
        })
    }
    #[inline]
    fn check(&self) -> Result<(), Error> {
        // Additional bounds checks are performed against the capability window in the handler.
        Ok(())
    }
}

#[derive(Copy, Clone)]
struct DeviceCapCreateArgsTyped {
    base: usize,
    len: usize,
    slot_raw: usize,
}

impl DeviceCapCreateArgsTyped {
    #[inline]
    fn decode(args: &Args) -> Result<Self, Error> {
        Ok(Self { base: args.get(0), len: args.get(1), slot_raw: args.get(2) })
    }
    #[inline]
    fn check(&self) -> Result<(), Error> {
        if self.len == 0 {
            return Err(AddressSpaceError::InvalidArgs.into());
        }
        if (self.base & (PAGE_SIZE - 1)) != 0 || (self.len & (PAGE_SIZE - 1)) != 0 {
            return Err(AddressSpaceError::InvalidArgs.into());
        }
        let end = self.base.checked_add(self.len).ok_or(AddressSpaceError::InvalidArgs)?;
        if end <= self.base {
            return Err(AddressSpaceError::InvalidArgs.into());
        }
        Ok(())
    }
}

#[derive(Copy, Clone)]
struct VmoCreateArgsTyped {
    slot_raw: usize,
    len: usize,
}

impl VmoCreateArgsTyped {
    #[inline]
    fn decode(args: &Args) -> Result<Self, Error> {
        Ok(Self { slot_raw: args.get(0), len: args.get(1) })
    }
    #[inline]
    fn check(&self) -> Result<(), Error> {
        if self.len == 0 {
            return Err(Error::Capability(CapError::PermissionDenied));
        }
        Ok(())
    }
}

#[derive(Copy, Clone)]
struct VmoWriteArgsTyped {
    slot: SlotIndex,
    offset: usize,
    user_ptr: usize,
    len: usize,
}

impl VmoWriteArgsTyped {
    #[inline]
    fn decode(args: &Args) -> Result<Self, Error> {
        Ok(Self {
            slot: SlotIndex::decode(args.get(0)),
            offset: args.get(1),
            user_ptr: args.get(2),
            len: args.get(3),
        })
    }
    #[inline]
    fn check(&self) -> Result<(), Error> {
        Ok(())
    }
}

#[derive(Copy, Clone)]
struct CapTransferArgsTyped {
    child: task::Pid,
    parent_slot: SlotIndex,
    rights_bits: u32,
}

impl CapTransferArgsTyped {
    #[inline]
    fn decode(args: &Args) -> Result<Self, Error> {
        Ok(Self {
            child: task::Pid::from_raw(args.get(0) as u32),
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

#[derive(Copy, Clone)]
struct CapTransferToArgsTyped {
    child: task::Pid,
    parent_slot: SlotIndex,
    rights_bits: u32,
    child_slot: SlotIndex,
}

impl CapTransferToArgsTyped {
    #[inline]
    fn decode(args: &Args) -> Result<Self, Error> {
        Ok(Self {
            child: task::Pid::from_raw(args.get(0) as u32),
            parent_slot: SlotIndex::decode(args.get(1)),
            rights_bits: args.get(2) as u32,
            child_slot: SlotIndex::decode(args.get(3)),
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
    Args, Error, SysResult, SyscallTable, SYSCALL_AS_CREATE, SYSCALL_AS_MAP, SYSCALL_CAP_QUERY,
    SYSCALL_CAP_TRANSFER, SYSCALL_CAP_TRANSFER_TO, SYSCALL_DEBUG_PUTC, SYSCALL_DEVICE_CAP_CREATE,
    SYSCALL_EXEC, SYSCALL_EXEC_V2, SYSCALL_EXIT, SYSCALL_IPC_ENDPOINT_CREATE, SYSCALL_IPC_RECV_V1,
    SYSCALL_IPC_SEND_V1, SYSCALL_MAP, SYSCALL_MMIO_MAP, SYSCALL_NSEC, SYSCALL_RECV, SYSCALL_SEND,
    SYSCALL_SPAWN, SYSCALL_SPAWN_LAST_ERROR, SYSCALL_VMO_CREATE, SYSCALL_VMO_WRITE, SYSCALL_WAIT,
    SYSCALL_YIELD,
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

fn wake_expired_blocked(ctx: &mut Context<'_>) {
    let now = ctx.timer.now();
    let len = ctx.tasks.len();
    for pid_usize in 0..len {
        let pid = task::Pid::from_raw(pid_usize as u32);
        let Some(t) = ctx.tasks.task(pid) else {
            continue;
        };
        if !t.is_blocked() {
            continue;
        }
        match t.block_reason() {
            Some(BlockReason::IpcRecv { endpoint, deadline_ns })
                if deadline_ns != 0 && now >= deadline_ns =>
            {
                let _ = ctx.router.remove_recv_waiter(endpoint, pid.as_raw());
                let _ = ctx.tasks.wake(pid, ctx.scheduler);
            }
            Some(BlockReason::IpcSend { endpoint, deadline_ns })
                if deadline_ns != 0 && now >= deadline_ns =>
            {
                let _ = ctx.router.remove_send_waiter(endpoint, pid.as_raw());
                let _ = ctx.tasks.wake(pid, ctx.scheduler);
            }
            _ => {}
        }
    }
}

/// Registers the default set of syscall handlers.
pub fn install_handlers(table: &mut SyscallTable) {
    table.register(SYSCALL_YIELD, sys_yield);
    table.register(SYSCALL_NSEC, sys_nsec);
    table.register(SYSCALL_SEND, sys_send);
    table.register(SYSCALL_RECV, sys_recv);
    table.register(SYSCALL_MAP, sys_map);
    table.register(SYSCALL_MMIO_MAP, sys_mmio_map);
    table.register(SYSCALL_CAP_QUERY, sys_cap_query);
    table.register(SYSCALL_DEVICE_CAP_CREATE, sys_device_cap_create);
    table.register(SYSCALL_VMO_CREATE, sys_vmo_create);
    table.register(SYSCALL_VMO_WRITE, sys_vmo_write);
    table.register(SYSCALL_SPAWN, sys_spawn);
    table.register(SYSCALL_CAP_TRANSFER, sys_cap_transfer);
    table.register(SYSCALL_CAP_TRANSFER_TO, sys_cap_transfer_to);
    table.register(SYSCALL_AS_CREATE, sys_as_create);
    table.register(SYSCALL_AS_MAP, sys_as_map);
    table.register(SYSCALL_EXIT, sys_exit);
    table.register(SYSCALL_WAIT, sys_wait);
    table.register(SYSCALL_EXEC, sys_exec);
    table.register(SYSCALL_IPC_SEND_V1, sys_ipc_send_v1);
    table.register(SYSCALL_EXEC_V2, sys_exec_v2);
    table.register(SYSCALL_IPC_RECV_V1, sys_ipc_recv_v1);
    table.register(SYSCALL_IPC_ENDPOINT_CREATE, sys_ipc_endpoint_create);
    table.register(crate::syscall::SYSCALL_CAP_CLOSE, sys_cap_close);
    table.register(crate::syscall::SYSCALL_CAP_CLONE, sys_cap_clone);
    table.register(crate::syscall::SYSCALL_IPC_ENDPOINT_CLOSE, sys_ipc_endpoint_close);
    table.register(crate::syscall::SYSCALL_IPC_ENDPOINT_CREATE_V2, sys_ipc_endpoint_create_v2);
    table.register(crate::syscall::SYSCALL_IPC_ENDPOINT_CREATE_FOR, sys_ipc_endpoint_create_for);
    table.register(crate::syscall::SYSCALL_GETPID, sys_getpid);
    table.register(crate::syscall::SYSCALL_IPC_RECV_V2, sys_ipc_recv_v2);
    table.register(SYSCALL_SPAWN_LAST_ERROR, sys_spawn_last_error);
    table.register(SYSCALL_DEBUG_PUTC, sys_debug_putc);
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        use core::fmt::Write as _;
        let mut u = crate::uart::raw_writer();
        let _ = u.write_str("SYSCALL install debug_putc=0x");
        crate::trap::uart_write_hex(&mut u, sys_debug_putc as usize);
        let _ = u.write_str("\n");
        let _ = u.write_str("SYSCALL install ep_create=0x");
        if let Some(addr) = table.debug_handler_addr(SYSCALL_IPC_ENDPOINT_CREATE) {
            crate::trap::uart_write_hex(&mut u, addr);
        } else {
            let _ = u.write_str("none");
        }
        let _ = u.write_str("\n");
    }
}

fn sys_getpid(ctx: &mut Context<'_>, _args: &Args) -> SysResult<usize> {
    Ok(ctx.tasks.current_pid().as_index())
}

fn sys_spawn_last_error(ctx: &mut Context<'_>, _args: &Args) -> SysResult<usize> {
    let pid = ctx.tasks.current_pid();
    let reason =
        ctx.tasks.take_last_spawn_fail_reason(pid).unwrap_or(crate::task::SpawnFailReason::Unknown);
    Ok(reason.as_u8() as usize)
}

fn sys_ipc_endpoint_create(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    // Deprecated ABI: keep deterministic failure (use v2 with endpoint-factory cap).
    let _ = (ctx, args);
    Err(Error::Capability(CapError::PermissionDenied))
}

fn sys_ipc_endpoint_create_v2(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let factory_slot = args.get(0);
    let depth = args.get(1);
    if depth == 0 || depth > 256 {
        return Err(AddressSpaceError::InvalidArgs.into());
    }
    let current = ctx.tasks.current_pid();
    let cap_table =
        ctx.tasks.caps_of(current).ok_or(Error::Capability(CapError::PermissionDenied))?;
    let cap = cap_table.get(factory_slot)?;
    if cap.kind != CapabilityKind::EndpointFactory || !cap.rights.contains(Rights::MANAGE) {
        return Err(Error::Capability(CapError::PermissionDenied));
    }
    let id = ctx.router.create_endpoint(depth, Some(current.as_raw()))?;
    let ep_cap = Capability {
        kind: CapabilityKind::Endpoint(id),
        rights: Rights::SEND | Rights::RECV | Rights::MANAGE,
    };
    let slot = ctx.tasks.current_caps_mut().allocate(ep_cap)?;
    Ok(slot)
}

fn sys_ipc_endpoint_create_for(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let factory_slot = args.get(0);
    let owner_pid = task::Pid::from_raw(args.get(1) as u32);
    let depth = args.get(2);
    if depth == 0 || depth > 256 {
        return Err(AddressSpaceError::InvalidArgs.into());
    }

    // Validate factory authority in the current task.
    let current = ctx.tasks.current_pid();
    let cap_table =
        ctx.tasks.caps_of(current).ok_or(Error::Capability(CapError::PermissionDenied))?;
    let cap = cap_table.get(factory_slot)?;
    if cap.kind != CapabilityKind::EndpointFactory || !cap.rights.contains(Rights::MANAGE) {
        return Err(Error::Capability(CapError::PermissionDenied));
    }

    // Validate the target owner exists.
    if ctx.tasks.task(owner_pid).is_none() {
        return Err(Error::Capability(CapError::PermissionDenied));
    }

    // Phase-2 hardening (authority tightening):
    // Even with an EndpointFactory, a task may only create endpoints owned by itself or by one of
    // its direct children. This prevents a compromised factory-holder from minting endpoints on
    // behalf of unrelated PIDs.
    if owner_pid != current {
        let parent = ctx.tasks.task(owner_pid).and_then(|t| t.parent());
        if parent != Some(current) {
            return Err(Error::Capability(CapError::PermissionDenied));
        }
    }

    let id = ctx.router.create_endpoint(depth, Some(owner_pid.as_raw()))?;
    #[cfg(feature = "ipc_trace_ring")]
    {
        crate::ipc::trace::record_ep_create(
            ctx.tasks.current_pid().as_raw(),
            id,
            depth as u16,
            owner_pid.as_raw() as u16,
        );
    }
    let ep_cap = Capability {
        kind: CapabilityKind::Endpoint(id),
        rights: Rights::SEND | Rights::RECV | Rights::MANAGE,
    };
    let slot = ctx.tasks.current_caps_mut().allocate(ep_cap)?;
    Ok(slot)
}
fn sys_cap_close(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let slot = args.get(0);
    // Local drop only: remove the capability slot from the caller.
    //
    // Global endpoint close is handled by `sys_ipc_endpoint_close` (requires `Rights::MANAGE`).
    let _ = ctx.tasks.current_caps_mut().take(slot)?;
    Ok(0)
}

fn sys_cap_clone(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let slot = args.get(0);
    let cap = ctx.tasks.current_caps_mut().get(slot)?;
    // Security floor: EndpointFactory must not be duplicable.
    if cap.kind == CapabilityKind::EndpointFactory {
        return Err(Error::Capability(CapError::PermissionDenied));
    }
    let new_slot = ctx.tasks.current_caps_mut().allocate(cap)?;
    Ok(new_slot)
}

fn sys_ipc_endpoint_close(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let slot = args.get(0);
    let cap = ctx.tasks.current_caps_mut().take(slot)?;
    let CapabilityKind::Endpoint(id) = cap.kind else {
        return Err(Error::Capability(CapError::InvalidSlot));
    };
    if !cap.rights.contains(Rights::MANAGE) {
        return Err(Error::Capability(CapError::PermissionDenied));
    }
    #[cfg(feature = "ipc_trace_ring")]
    {
        crate::ipc::trace::record_ep_close(ctx.tasks.current_pid().as_raw(), id);
    }
    let waiters = ctx.router.close_endpoint(id)?;
    for pid in waiters {
        let _ = ctx.tasks.wake(task::Pid::from_raw(pid), ctx.scheduler);
    }
    Ok(0)
}

fn sys_yield(ctx: &mut Context<'_>, _args: &Args) -> SysResult<usize> {
    crate::liveness::bump();
    ctx.scheduler.yield_current();
    if let Some(next) = ctx.scheduler.schedule_next() {
        ctx.tasks.set_current(next);
        if let Some(task) = ctx.tasks.task(next) {
            #[cfg(feature = "debug_uart")]
            {
                use core::fmt::Write as _;
                let mut w = crate::uart::raw_writer();
                let _ = write!(w, "YIELD-I: next pid={} sepc=0x{:x}\n", next, task.frame().sepc);
            }
            #[cfg(not(feature = "debug_uart"))]
            let _ = task; // silence unused when debug UART is disabled
        }
        Ok(next.as_index())
    } else {
        Ok(ctx.tasks.current_pid().as_index())
    }
}

fn sys_nsec(ctx: &mut Context<'_>, _args: &Args) -> SysResult<usize> {
    Ok(ctx.timer.now() as usize)
}

fn sys_send(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let typed = SendArgsTyped::decode(args)?;
    typed.check()?;
    let endpoint =
        ctx.tasks.current_caps_mut().derive_endpoint_ref(typed.slot.0, Rights::SEND)?.endpoint();
    let header =
        MessageHeader::new(typed.slot.0 as u32, endpoint, typed.ty, typed.flags, typed.len);
    let payload = Vec::new();
    ctx.router.send(endpoint, ipc::Message::new(header, payload, None))?;
    Ok(typed.len as usize)
}

fn sys_recv(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let typed = RecvArgsTyped::decode(args)?;
    typed.check()?;
    let endpoint =
        ctx.tasks.current_caps_mut().derive_endpoint_ref(typed.slot.0, Rights::RECV)?.endpoint();
    let message = ctx.router.recv(endpoint)?;
    let len = message.header.len as usize;
    ctx.last_message = Some(message);
    Ok(len)
}

fn sys_ipc_send_v1(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let typed = IpcSendV1ArgsTyped::decode(args)?;
    typed.check()?;

    if (typed.sys_flags & !(IPC_SYS_NONBLOCK)) != 0 {
        return Err(AddressSpaceError::InvalidArgs.into());
    }
    let nonblock = (typed.sys_flags & IPC_SYS_NONBLOCK) != 0;

    let endpoint =
        ctx.tasks.current_caps_mut().derive_endpoint_ref(typed.slot.0, Rights::SEND)?.endpoint();

    let mut hdr_bytes = [0u8; 16];
    unsafe {
        core::ptr::copy_nonoverlapping(
            typed.header_ptr as *const u8,
            hdr_bytes.as_mut_ptr(),
            hdr_bytes.len(),
        );
    }
    let user_hdr = MessageHeader::from_le_bytes(hdr_bytes);

    // IPC v1 extension: move one capability alongside the message.
    // When set, `user_hdr.src` is treated as a cap slot in the sender, which is consumed (taken)
    // and delivered to the receiver. On receive, `header_out.src` is overwritten with the newly
    // allocated cap slot in the receiver.
    const IPC_MSG_FLAG_CAP_MOVE: u16 = 1 << 0;
    let cap_move = (user_hdr.flags & IPC_MSG_FLAG_CAP_MOVE) != 0;

    // Enforce header/payload agreement.
    if user_hdr.len as usize != typed.payload_len {
        return Err(AddressSpaceError::InvalidArgs.into());
    }

    let mut payload = Vec::new();
    if typed.payload_len != 0 {
        payload.resize(typed.payload_len, 0);
        unsafe {
            core::ptr::copy_nonoverlapping(
                typed.payload_ptr as *const u8,
                payload.as_mut_ptr(),
                typed.payload_len,
            );
        }
    }

    let cap_move_slot = if cap_move { Some(user_hdr.src as usize) } else { None };

    // Sender attribution: kernel sets `dst` to the sender PID so receivers can attribute messages.
    // `src` is reserved for CAP_MOVE return value on receive.
    let header = MessageHeader::new(
        0,
        ctx.tasks.current_pid().as_raw(),
        user_hdr.ty,
        user_hdr.flags,
        typed.payload_len as u32,
    );

    if !nonblock && typed.deadline_ns != 0 {
        ctx.timer.set_wakeup(typed.deadline_ns);
    }
    loop {
        // If CAP_MOVE is set, take the cap for this attempt. If the attempt fails (QueueFull,
        // NoSuchEndpoint, etc.) we restore it before returning/rescheduling.
        let moved_cap = if let Some(slot) = cap_move_slot {
            let cap = ctx.tasks.current_caps_mut().take(slot)?;
            // Security floor: never allow moving MANAGE authority in-band.
            if cap.rights.contains(Rights::MANAGE) || cap.kind == CapabilityKind::EndpointFactory {
                let _ = ctx.tasks.current_caps_mut().set(slot, cap);
                return Err(Error::Capability(CapError::PermissionDenied));
            }
            // Hardening: do not allow CAP_MOVE of dead/non-existent endpoints.
            if let CapabilityKind::Endpoint(id) = cap.kind {
                if !ctx.router.endpoint_alive(id) {
                    let _ = ctx.tasks.current_caps_mut().set(slot, cap);
                    return Err(Error::Ipc(ipc::IpcError::NoSuchEndpoint));
                }
                #[cfg(feature = "ipc_trace_ring")]
                {
                    crate::ipc::trace::record_capmove_send(
                        ctx.tasks.current_pid().as_raw(),
                        typed.slot.0 as u16,
                        slot as u16,
                        endpoint,
                        id,
                    );
                }
            }
            Some(cap)
        } else {
            None
        };
        let mut msg = ipc::Message::new(header, payload.clone(), moved_cap);
        if cap_move {
            if let Some(cap) = msg.moved_cap {
                if let CapabilityKind::Endpoint(id) = cap.kind {
                    msg.capmove_expected_ep = id;
                }
            }
        }
        msg.sender_service_id = ctx.tasks.current_service_id();
        #[cfg(feature = "ipc_trace_ring")]
        {
            crate::ipc::trace::record_send(
                ctx.tasks.current_pid().as_raw(),
                typed.slot.0 as u16,
                endpoint,
                msg.header.flags,
                msg.payload.len() as u16,
                None,
            );
        }
        #[cfg(feature = "debug_uart")]
        {
            if payload.len() >= 4
                && payload[0] == b'S'
                && payload[1] == b'M'
                && payload[2] == 1
                && payload[3] == 1
            {
                use core::fmt::Write as _;
                let mut u = crate::uart::raw_writer();
                let _ = writeln!(u, "IPC-SEND samgr reg ep=0x{:x}", endpoint);
            }
        }
        match ctx.router.send_returning_message(endpoint, msg) {
            Ok(()) => {
                // Wake one receiver blocked on this endpoint (if any).
                if let Ok(Some(waiter)) = ctx.router.pop_recv_waiter(endpoint) {
                    let _ = ctx.tasks.wake(task::Pid::from_raw(waiter), ctx.scheduler);
                }
                // Low-noise triage: dump trace ring once on the first "large CAP_MOVE" send.
                // This helps diagnose OTA stage hangs without relying on NoSuchEndpoint spam.
                #[cfg(feature = "ipc_trace_ring")]
                if cap_move && typed.payload_len > 1024 {
                    crate::ipc::trace::maybe_dump_capmove_big("capmove-big");
                }
                return Ok(typed.payload_len);
            }
            Err((ipc::IpcError::QueueFull, msg)) if !nonblock => {
                // Roll back moved cap before blocking/rescheduling.
                if let Some(slot) = cap_move_slot {
                    if let Some(cap) = msg.moved_cap {
                        let _ = ctx.tasks.current_caps_mut().set(slot, cap);
                    }
                }
                if typed.deadline_ns != 0 && ctx.timer.now() >= typed.deadline_ns {
                    return Err(Error::Ipc(ipc::IpcError::TimedOut));
                }
                let cur = ctx.tasks.current_pid();
                // IMPORTANT: if the endpoint is gone, do not block (would deadlock forever).
                ctx.router.register_send_waiter(endpoint, cur.as_raw())?;
                ctx.tasks.block_current(
                    BlockReason::IpcSend { endpoint, deadline_ns: typed.deadline_ns },
                    ctx.scheduler,
                );
                wake_expired_blocked(ctx);
                if let Some(next) = ctx.scheduler.schedule_next() {
                    ctx.tasks.set_current(next);
                    return Err(Error::Reschedule);
                }
                // Degenerate fallback: nothing runnable. Undo waiter registration and keep spinning.
                let _ = ctx.router.remove_send_waiter(endpoint, cur.as_raw());
                let _ = ctx.tasks.wake(cur, ctx.scheduler);
                return Err(Error::Reschedule);
            }
            Err((e, msg)) => {
                // Roll back the moved cap on any error so the caller does not lose it.
                if let Some(slot) = cap_move_slot {
                    if let Some(cap) = msg.moved_cap {
                        let _ = ctx.tasks.current_caps_mut().set(slot, cap);
                    }
                }
                #[cfg(feature = "ipc_trace_ring")]
                {
                    crate::ipc::trace::record_send(
                        ctx.tasks.current_pid().as_raw(),
                        typed.slot.0 as u16,
                        endpoint,
                        msg.header.flags,
                        msg.payload.len() as u16,
                        Some(e),
                    );
                    if e == ipc::IpcError::NoSuchEndpoint {
                        crate::ipc::trace::dump_uart_send_nosuch(endpoint);
                    }
                }
                #[cfg(feature = "debug_uart")]
                if e == ipc::IpcError::NoSuchEndpoint {
                    use core::fmt::Write as _;
                    let mut u = crate::uart::raw_writer();
                    let _ = writeln!(
                        u,
                        "IPC-SEND nosuch ep=0x{:x} flags=0x{:x} capmove={}",
                        endpoint,
                        msg.header.flags,
                        msg.moved_cap.is_some()
                    );
                }
                return Err(e.into());
            }
        }
    }
}

fn sys_ipc_recv_v1(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let typed = IpcRecvV1ArgsTyped::decode(args)?;
    typed.check()?;

    if (typed.sys_flags & !(IPC_SYS_NONBLOCK | IPC_SYS_TRUNCATE)) != 0 {
        return Err(AddressSpaceError::InvalidArgs.into());
    }

    let endpoint =
        ctx.tasks.current_caps_mut().derive_endpoint_ref(typed.slot.0, Rights::RECV)?.endpoint();

    let truncate = (typed.sys_flags & IPC_SYS_TRUNCATE) != 0;
    let nonblock = (typed.sys_flags & IPC_SYS_NONBLOCK) != 0;
    if !nonblock && typed.deadline_ns != 0 {
        ctx.timer.set_wakeup(typed.deadline_ns);
    }
    let mut msg = loop {
        match ctx.router.recv(endpoint) {
            Ok(msg) => {
                // Receiving frees queue capacity; wake one sender blocked on this endpoint (if any).
                if let Ok(Some(waiter)) = ctx.router.pop_send_waiter(endpoint) {
                    let _ = ctx.tasks.wake(task::Pid::from_raw(waiter), ctx.scheduler);
                }
                break msg;
            }
            Err(ipc::IpcError::QueueEmpty) if !nonblock => {
                if typed.deadline_ns != 0 && ctx.timer.now() >= typed.deadline_ns {
                    return Err(Error::Ipc(ipc::IpcError::TimedOut));
                }
                let cur = ctx.tasks.current_pid();
                // IMPORTANT: if the endpoint is gone, do not block (would deadlock forever).
                ctx.router.register_recv_waiter(endpoint, cur.as_raw())?;
                // Avoid missed-wakeup: a sender can enqueue between our empty check and waiter
                // registration. Re-check once after registering; if a message is present, consume
                // it without blocking.
                match ctx.router.recv(endpoint) {
                    Ok(msg) => {
                        let _ = ctx.router.remove_recv_waiter(endpoint, cur.as_raw());
                        if let Ok(Some(waiter)) = ctx.router.pop_send_waiter(endpoint) {
                            let _ = ctx.tasks.wake(task::Pid::from_raw(waiter), ctx.scheduler);
                        }
                        break msg;
                    }
                    Err(ipc::IpcError::QueueEmpty) => {
                        // Proceed to block below.
                    }
                    Err(e) => {
                        let _ = ctx.router.remove_recv_waiter(endpoint, cur.as_raw());
                        return Err(e.into());
                    }
                }
                ctx.tasks.block_current(
                    BlockReason::IpcRecv { endpoint, deadline_ns: typed.deadline_ns },
                    ctx.scheduler,
                );
                wake_expired_blocked(ctx);
                if let Some(next) = ctx.scheduler.schedule_next() {
                    ctx.tasks.set_current(next);
                    return Err(Error::Reschedule);
                }
                // Degenerate fallback: nothing runnable. Undo waiter registration and keep spinning.
                let _ = ctx.router.remove_recv_waiter(endpoint, cur.as_raw());
                let _ = ctx.tasks.wake(cur, ctx.scheduler);
                return Err(Error::Reschedule);
            }
            Err(e) => return Err(e.into()),
        }
    };
    #[cfg(feature = "ipc_trace_ring")]
    {
        crate::ipc::trace::record_recv(
            ctx.tasks.current_pid().as_raw(),
            typed.slot.0 as u16,
            endpoint,
            msg.header.flags,
            msg.payload.len() as u16,
            None,
        );
    }

    // If the message carries a moved capability, allocate it into the receiver now and write the
    // allocated slot into the returned header's `src` field.
    if let Some(mut cap) = msg.moved_cap.take() {
        // CAP_MOVE robustness: if the moved endpoint id is inconsistent with what the sender
        // observed, prefer the sender's value (kernel internal field).
        if msg.capmove_expected_ep != 0 {
            if let CapabilityKind::Endpoint(id) = cap.kind {
                if id != msg.capmove_expected_ep {
                    #[cfg(feature = "ipc_trace_ring")]
                    {
                        use core::fmt::Write as _;
                        let mut u = crate::uart::raw_writer();
                        let _ = writeln!(
                            u,
                            "IPC-CAPMOVE fix exp=0x{:x} got=0x{:x}",
                            msg.capmove_expected_ep, id
                        );
                    }
                    cap.kind = CapabilityKind::Endpoint(msg.capmove_expected_ep);
                }
            }
        }
        #[cfg(feature = "ipc_trace_ring")]
        let moved_ep_for_trace: u32 = match cap.kind {
            CapabilityKind::Endpoint(id) => id,
            _ => 0,
        };
        #[cfg(feature = "debug_uart")]
        {
            use core::fmt::Write as _;
            let mut u = crate::uart::raw_writer();
            let mut cap_info = 0usize;
            if let CapabilityKind::Endpoint(id) = cap.kind {
                cap_info = id as usize;
            }
            let _ = writeln!(u, "IPC-CAPMOVE recv ep=0x{:x} cap_ep=0x{:x}", endpoint, cap_info);
        }
        match ctx.tasks.current_caps_mut().allocate(cap) {
            Ok(slot) => {
                #[cfg(feature = "ipc_trace_ring")]
                {
                    crate::ipc::trace::record_capmove_alloc(
                        ctx.tasks.current_pid().as_raw(),
                        endpoint,
                        slot as u32,
                        moved_ep_for_trace,
                    );
                }
                #[cfg(feature = "debug_uart")]
                {
                    use core::fmt::Write as _;
                    let mut u = crate::uart::raw_writer();
                    let _ = writeln!(u, "IPC-CAPMOVE recv slot=0x{:x}", slot);
                }
                msg.header.src = slot as u32;
                // Low-noise triage: dump trace ring once on the first "large CAP_MOVE" receive
                // after the capability has been allocated (so we can correlate with the sender dump).
                #[cfg(feature = "ipc_trace_ring")]
                if msg.header.len > 1024 {
                    crate::ipc::trace::maybe_dump_capmove_big_recv("capmove-big-recv");
                }
            }
            Err(_) => {
                #[cfg(feature = "debug_uart")]
                {
                    use core::fmt::Write as _;
                    let mut u = crate::uart::raw_writer();
                    let _ = writeln!(u, "IPC-CAPMOVE recv nospace");
                }
                // Roll back: receiver cannot accept a moved cap right now (e.g. no free cap slots).
                // Re-queue the message and surface a stable syscall error (ENOSPC).
                msg.moved_cap = Some(cap);
                let _ = ctx.router.requeue_front(endpoint, msg);
                return Err(Error::Ipc(ipc::IpcError::NoSpace));
            }
        }
    }

    // Copy-out header (always).
    let hdr = msg.header.to_le_bytes();
    unsafe {
        core::ptr::copy_nonoverlapping(hdr.as_ptr(), typed.header_out_ptr as *mut u8, hdr.len());
    }

    let total = msg.payload.len();
    if total == 0 || typed.payload_out_max == 0 {
        ctx.last_message = Some(msg);
        return Ok(0);
    }

    if total > typed.payload_out_max && !truncate {
        return Err(AddressSpaceError::InvalidArgs.into());
    }

    let n = core::cmp::min(total, typed.payload_out_max);
    unsafe {
        core::ptr::copy_nonoverlapping(msg.payload.as_ptr(), typed.payload_out_ptr as *mut u8, n);
    }

    ctx.last_message = Some(msg);
    Ok(n)
}

// IPC recv v2: descriptor-based syscall to return additional sender identity metadata without
// being limited by a0-a5 register count.
//
// Descriptor layout is versioned to keep the ABI extensible.
const IPC_RECV_V2_MAGIC: u32 = 0x4E_58_49_32; // 'N''X''I''2'
const IPC_RECV_V2_VERSION: u32 = 1;

#[repr(C)]
#[derive(Clone, Copy)]
struct IpcRecvV2Desc {
    magic: u32,
    version: u32,
    slot: u32,
    _pad0: u32,
    header_out_ptr: u64,
    payload_out_ptr: u64,
    payload_out_max: u64,
    sender_service_id_out_ptr: u64,
    sys_flags: u32,
    _pad1: u32,
    deadline_ns: u64,
}

fn sys_ipc_recv_v2(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let desc_ptr = args.get(0);
    // Defensive: require the descriptor itself to be a valid user slice.
    ensure_user_slice(desc_ptr, core::mem::size_of::<IpcRecvV2Desc>())?;
    let mut raw = [0u8; core::mem::size_of::<IpcRecvV2Desc>()];
    unsafe {
        core::ptr::copy_nonoverlapping(desc_ptr as *const u8, raw.as_mut_ptr(), raw.len());
    }

    let magic = read_u32_le(&raw, 0)?;
    let version = read_u32_le(&raw, 4)?;
    if magic != IPC_RECV_V2_MAGIC || version != IPC_RECV_V2_VERSION {
        return Err(AddressSpaceError::InvalidArgs.into());
    }
    let slot = read_u32_le(&raw, 8)? as u32;
    let header_out_ptr = read_u64_le(&raw, 16)? as usize;
    let payload_out_ptr = read_u64_le(&raw, 24)? as usize;
    let payload_out_max = read_u64_le(&raw, 32)? as usize;
    let sender_service_id_out_ptr = read_u64_le(&raw, 40)? as usize;
    let sys_flags = read_u32_le(&raw, 48)? as usize;
    let deadline_ns = read_u64_le(&raw, 56)?;

    // Validate pointers up-front (RFC-0004 style provenance).
    ensure_user_slice(header_out_ptr, 16)?;
    const MAX_FRAME_BYTES: usize = 8 * 1024;
    if payload_out_max > MAX_FRAME_BYTES {
        return Err(AddressSpaceError::InvalidArgs.into());
    }
    if payload_out_max != 0 {
        ensure_user_slice(payload_out_ptr, payload_out_max)?;
    }
    ensure_user_slice(sender_service_id_out_ptr, 8)?;

    if (sys_flags & !(IPC_SYS_NONBLOCK | IPC_SYS_TRUNCATE)) != 0 {
        return Err(AddressSpaceError::InvalidArgs.into());
    }

    // Derive endpoint.
    let endpoint =
        ctx.tasks.current_caps_mut().derive_endpoint_ref(slot as usize, Rights::RECV)?.endpoint();

    let truncate = (sys_flags & IPC_SYS_TRUNCATE) != 0;
    let nonblock = (sys_flags & IPC_SYS_NONBLOCK) != 0;
    if !nonblock && deadline_ns != 0 {
        ctx.timer.set_wakeup(deadline_ns);
    }

    let mut msg = loop {
        match ctx.router.recv(endpoint) {
            Ok(msg) => {
                if let Ok(Some(waiter)) = ctx.router.pop_send_waiter(endpoint) {
                    let _ = ctx.tasks.wake(task::Pid::from_raw(waiter), ctx.scheduler);
                }
                break msg;
            }
            Err(ipc::IpcError::QueueEmpty) if !nonblock => {
                if deadline_ns != 0 && ctx.timer.now() >= deadline_ns {
                    return Err(Error::Ipc(ipc::IpcError::TimedOut));
                }
                let cur = ctx.tasks.current_pid();
                ctx.router.register_recv_waiter(endpoint, cur.as_raw())?;
                // Avoid missed-wakeup: re-check after registering.
                match ctx.router.recv(endpoint) {
                    Ok(msg) => {
                        let _ = ctx.router.remove_recv_waiter(endpoint, cur.as_raw());
                        if let Ok(Some(waiter)) = ctx.router.pop_send_waiter(endpoint) {
                            let _ = ctx.tasks.wake(task::Pid::from_raw(waiter), ctx.scheduler);
                        }
                        break msg;
                    }
                    Err(ipc::IpcError::QueueEmpty) => {}
                    Err(e) => {
                        let _ = ctx.router.remove_recv_waiter(endpoint, cur.as_raw());
                        return Err(e.into());
                    }
                }
                ctx.tasks
                    .block_current(BlockReason::IpcRecv { endpoint, deadline_ns }, ctx.scheduler);
                wake_expired_blocked(ctx);
                if let Some(next) = ctx.scheduler.schedule_next() {
                    ctx.tasks.set_current(next);
                    return Err(Error::Reschedule);
                }
                let _ = ctx.router.remove_recv_waiter(endpoint, cur.as_raw());
                let _ = ctx.tasks.wake(cur, ctx.scheduler);
                return Err(Error::Reschedule);
            }
            Err(e) => return Err(e.into()),
        }
    };

    // CAP_MOVE allocation (same semantics as v1).
    if let Some(mut cap) = msg.moved_cap.take() {
        if msg.capmove_expected_ep != 0 {
            if let CapabilityKind::Endpoint(id) = cap.kind {
                if id != msg.capmove_expected_ep {
                    cap.kind = CapabilityKind::Endpoint(msg.capmove_expected_ep);
                }
            }
        }
        match ctx.tasks.current_caps_mut().allocate(cap) {
            Ok(slot) => {
                msg.header.src = slot as u32;
            }
            Err(_) => {
                msg.moved_cap = Some(cap);
                let _ = ctx.router.requeue_front(endpoint, msg);
                return Err(Error::Ipc(ipc::IpcError::NoSpace));
            }
        }
    }

    // Copy-out header.
    let hdr = msg.header.to_le_bytes();
    unsafe {
        core::ptr::copy_nonoverlapping(hdr.as_ptr(), header_out_ptr as *mut u8, hdr.len());
    }

    // Copy-out sender service id (kernel-derived).
    let sid = msg.sender_service_id.to_le_bytes();
    unsafe {
        core::ptr::copy_nonoverlapping(
            sid.as_ptr(),
            sender_service_id_out_ptr as *mut u8,
            sid.len(),
        );
    }

    let total = msg.payload.len();
    if total == 0 || payload_out_max == 0 {
        ctx.last_message = Some(msg);
        return Ok(0);
    }
    if total > payload_out_max && !truncate {
        return Err(AddressSpaceError::InvalidArgs.into());
    }
    let n = core::cmp::min(total, payload_out_max);
    unsafe {
        core::ptr::copy_nonoverlapping(msg.payload.as_ptr(), payload_out_ptr as *mut u8, n);
    }
    ctx.last_message = Some(msg);
    Ok(n)
}

fn sys_map(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let typed = MapArgsTyped::decode(args)?;
    typed.check()?;
    let cap = ctx.tasks.current_caps_mut().derive(typed.slot.0, Rights::MAP)?;
    match cap.kind {
        CapabilityKind::Vmo { base, len } => {
            if typed.offset >= len {
                return Err(Error::Capability(CapError::PermissionDenied));
            }
            let va = typed.va;
            let pa = base + (typed.offset & !0xfff);
            let handle =
                ctx.tasks.current_task().address_space().ok_or(AddressSpaceError::InvalidHandle)?;
            #[cfg(feature = "debug_uart")]
            {
                use core::fmt::Write as _;
                let mut u = crate::uart::raw_writer();
                let _ = writeln!(
                    u,
                    "AS-MAP handle=0x{:x} va=0x{:x} pa=0x{:x} flags=0x{:x}",
                    handle.to_raw(),
                    va.raw(),
                    pa,
                    typed.flags.bits()
                );
            }
            ctx.address_spaces.map_page(handle, va.raw(), pa, typed.flags)?;
            Ok(0)
        }
        _ => Err(Error::Capability(CapError::PermissionDenied)),
    }
}

fn sys_mmio_map(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let typed = MmioMapArgsTyped::decode(args)?;
    typed.check()?;

    let cap = ctx.tasks.current_caps_mut().derive(typed.slot.0, Rights::MAP)?;

    let (base, len) = match cap.kind {
        CapabilityKind::DeviceMmio { base, len } => (base, len),
        _ => return Err(Error::Capability(CapError::PermissionDenied)),
    };

    if typed.offset >= len {
        return Err(Error::Capability(CapError::PermissionDenied));
    }
    // Enforce page-granularity offsets (per normative v1 contract).
    if (typed.offset & (PAGE_SIZE - 1)) != 0 {
        return Err(Error::Capability(CapError::PermissionDenied));
    }

    let handle =
        ctx.tasks.current_task().address_space().ok_or(AddressSpaceError::InvalidHandle)?;

    // Enforce the security floor at the boundary:
    // - USER + RW only
    // - never EXEC
    let flags = PageFlags::VALID | PageFlags::USER | PageFlags::READ | PageFlags::WRITE;

    let pa =
        base.checked_add(typed.offset & !(PAGE_SIZE - 1)).ok_or(AddressSpaceError::InvalidArgs)?;

    ctx.address_spaces.map_page(handle, typed.va.raw(), pa, flags)?;
    Ok(0)
}

fn sys_device_cap_create(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let typed = DeviceCapCreateArgsTyped::decode(args)?;
    typed.check()?;

    // Privileged gate: require EndpointFactory with MANAGE (init-lite only).
    let factory_cap = ctx
        .tasks
        .current_caps_mut()
        .get(1)
        .map_err(|_| Error::Capability(CapError::PermissionDenied))?;
    if factory_cap.kind != CapabilityKind::EndpointFactory
        || !factory_cap.rights.contains(Rights::MANAGE)
    {
        return Err(Error::Capability(CapError::PermissionDenied));
    }

    let cap = Capability {
        kind: CapabilityKind::DeviceMmio { base: typed.base, len: typed.len },
        rights: Rights::MAP,
    };
    let slot = if typed.slot_raw == usize::MAX {
        ctx.tasks.current_caps_mut().allocate(cap)?
    } else {
        ctx.tasks.current_caps_mut().set(typed.slot_raw, cap)?;
        typed.slot_raw
    };
    Ok(slot)
}

fn sys_cap_query(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let slot = SlotIndex::decode(args.get(0));
    let out_ptr = args.get(1);
    // out layout (LE):
    // - u32 kind_tag (1=vmo, 2=device_mmio)
    // - u32 reserved
    // - u64 base
    // - u64 len
    const OUT_LEN: usize = 24;
    ensure_user_slice(out_ptr, OUT_LEN)?;

    // Capability gate: require MAP rights to introspect address-bearing caps.
    let cap = ctx.tasks.current_caps_mut().derive(slot.0, Rights::MAP)?;
    let (kind_tag, base, len) = match cap.kind {
        CapabilityKind::Vmo { base, len } => (1u32, base as u64, len as u64),
        CapabilityKind::DeviceMmio { base, len } => (2u32, base as u64, len as u64),
        _ => return Err(Error::Capability(CapError::PermissionDenied)),
    };

    let mut out = [0u8; OUT_LEN];
    out[0..4].copy_from_slice(&kind_tag.to_le_bytes());
    out[4..8].copy_from_slice(&0u32.to_le_bytes());
    out[8..16].copy_from_slice(&base.to_le_bytes());
    out[16..24].copy_from_slice(&len.to_le_bytes());
    unsafe {
        core::ptr::copy_nonoverlapping(out.as_ptr(), out_ptr as *mut u8, OUT_LEN);
    }
    Ok(0)
}

fn sys_vmo_create(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let typed = VmoCreateArgsTyped::decode(args)?;
    typed.check()?;
    let (base, aligned_len) = VMO_POOL.lock().allocate(typed.len)?;
    #[cfg(feature = "debug_uart")]
    {
        use core::fmt::Write as _;
        let mut u = crate::uart::raw_writer();
        let _ = writeln!(
            u,
            "VMO-CREATE len=0x{:x} base=0x{:x} slot=0x{:x}",
            aligned_len, base, typed.slot_raw
        );
    }
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
    let typed = VmoWriteArgsTyped::decode(args)?;
    typed.check()?;
    let cap = ctx.tasks.current_caps_mut().derive(typed.slot.0, Rights::MAP)?;
    let (base, vmo_len) = match cap.kind {
        CapabilityKind::Vmo { base, len } => (base, len),
        _ => return Err(Error::Capability(CapError::PermissionDenied)),
    };
    #[cfg(feature = "debug_uart")]
    {
        use core::fmt::Write as _;
        let mut u = crate::uart::raw_writer();
        let _ = write!(
            u,
            "VMO-WRITE slot=0x{:x} base=0x{:x} off=0x{:x} len=0x{:x} user=0x{:x}\n",
            typed.slot.0, base, typed.offset, typed.len, typed.user_ptr
        );
    }
    let span_end =
        typed.offset.checked_add(typed.len).ok_or(Error::Capability(CapError::PermissionDenied))?;
    if span_end > vmo_len {
        return Err(Error::Capability(CapError::PermissionDenied));
    }
    ensure_user_slice(typed.user_ptr, typed.len)?;
    #[cfg(feature = "debug_uart")]
    let preview_len = core::cmp::min(typed.len, 16);
    #[cfg(feature = "debug_uart")]
    let mut preview_bytes = [0u8; 16];
    #[cfg(feature = "debug_uart")]
    if preview_len > 0 {
        unsafe {
            ptr::copy_nonoverlapping(
                typed.user_ptr as *const u8,
                preview_bytes.as_mut_ptr(),
                preview_len,
            );
        }
        use core::fmt::Write as _;
        let mut u = crate::uart::raw_writer();
        let _ =
            write!(u, "VMO-WRITE DATA slot=0x{:x} off=0x{:x} head=0x", typed.slot.0, typed.offset);
        for byte in preview_bytes.iter().take(preview_len) {
            let _ = write!(u, "{:02x}", byte);
        }
        let _ = u.write_str("\n");
    }
    if typed.len != 0 {
        unsafe {
            ptr::copy_nonoverlapping(
                typed.user_ptr as *const u8,
                (base + typed.offset) as *mut u8,
                typed.len,
            );
            riscv::asm::fence_i();
        }
    }
    Ok(typed.len)
}

fn sys_exit(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let status = args.get(0) as i32;
    let exiting = ctx.tasks.current_pid();
    // RFC-0005 lifecycle: close endpoints owned by this task and wake any blocked peers.
    let waiters = ctx.router.close_endpoints_for_owner(exiting.as_raw());
    ctx.router.remove_waiter_from_all(exiting.as_raw());
    ctx.tasks.exit_current(status);
    for pid in waiters {
        let _ = ctx.tasks.wake(task::Pid::from_raw(pid), ctx.scheduler);
    }
    ctx.tasks.wake_parent_waiter(exiting, ctx.scheduler);
    ctx.scheduler.finish_current();
    if let Some(next) = ctx.scheduler.schedule_next() {
        ctx.tasks.set_current(next);
        if let Some(task) = ctx.tasks.task(next) {
            #[cfg(not(feature = "selftest_no_satp"))]
            {
                if let Some(handle) = task.address_space() {
                    ctx.address_spaces.activate(handle)?;
                }
            }
            #[cfg(feature = "selftest_no_satp")]
            let _ = task;
        }
    } else {
        ctx.tasks.set_current(task::Pid::KERNEL);
    }
    Err(Error::TaskExit)
}

fn sys_wait(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let raw_pid = args.get(0) as i32;
    let target =
        if raw_pid <= 0 { None } else { Some(task::Pid::from_raw(raw_pid as u32)) };
    loop {
        match ctx.tasks.reap_child(target, ctx.address_spaces) {
            Ok((pid, status)) => {
                if let Some(task) = ctx.tasks.task_mut(ctx.tasks.current_pid()) {
                    task.frame_mut().x[11] = status as usize;
                }
                return Ok(pid.as_index());
            }
            Err(task::WaitError::WouldBlock) => {
                let cur = ctx.tasks.current_pid();
                ctx.tasks.block_current(BlockReason::WaitChild { target }, ctx.scheduler);
                if let Some(next) = ctx.scheduler.schedule_next() {
                    ctx.tasks.set_current(next);
                    return Err(Error::Reschedule);
                }
                let _ = ctx.tasks.wake(cur, ctx.scheduler);
                return Err(Error::Reschedule);
            }
            Err(err) => return Err(Error::from(err)),
        }
    }
}

fn read_u16_le(bytes: &[u8], off: usize) -> Result<u16, Error> {
    let end = off.checked_add(2).ok_or(AddressSpaceError::InvalidArgs)?;
    let slice = bytes.get(off..end).ok_or(AddressSpaceError::InvalidArgs)?;
    Ok(u16::from_le_bytes([slice[0], slice[1]]))
}

fn read_u32_le(bytes: &[u8], off: usize) -> Result<u32, Error> {
    let end = off.checked_add(4).ok_or(AddressSpaceError::InvalidArgs)?;
    let slice = bytes.get(off..end).ok_or(AddressSpaceError::InvalidArgs)?;
    Ok(u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

fn read_u64_le(bytes: &[u8], off: usize) -> Result<u64, Error> {
    let end = off.checked_add(8).ok_or(AddressSpaceError::InvalidArgs)?;
    let slice = bytes.get(off..end).ok_or(AddressSpaceError::InvalidArgs)?;
    Ok(u64::from_le_bytes([
        slice[0], slice[1], slice[2], slice[3], slice[4], slice[5], slice[6], slice[7],
    ]))
}

/// Kernel-side exec loader: parses ELF64/RISC-V, maps PT_LOAD with W^X + USER, sets stack, spawns task.
fn sys_exec(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let typed = ExecArgsTyped::decode(args)?;
    typed.check()?;

    // Capability gate: only tasks holding a SEND right in slot 0 (bootstrap cap) may exec.
    {
        let _ = ctx
            .tasks
            .current_caps_mut()
            .derive(0, Rights::SEND)
            .map_err(|_| Error::Capability(CapError::PermissionDenied))?;
    }

    // SAFETY: user slice validated by check; still best-effort.
    let elf = unsafe { slice::from_raw_parts(typed.elf_ptr as *const u8, typed.elf_len) };
    if elf.len() < 64 || &elf[0..4] != b"\x7FELF" {
        return Err(AddressSpaceError::InvalidArgs.into());
    }
    if elf[4] != 2 || elf[5] != 1 {
        return Err(AddressSpaceError::InvalidArgs.into()); // not ELF64/LE
    }

    let e_entry = read_u64_le(elf, 24)? as usize;
    let e_phoff = read_u64_le(elf, 32)? as usize;
    let e_phentsize = read_u16_le(elf, 54)? as usize;
    let e_phnum = read_u16_le(elf, 56)? as usize;
    if e_phoff >= elf.len() {
        return Err(AddressSpaceError::InvalidArgs.into());
    }
    {
        use core::fmt::Write as _;
        let mut u = crate::uart::raw_writer();
        let _ = write!(
            u,
            "[INFO exec] EXEC-ELF hdr entry=0x{:x} phoff=0x{:x} phentsz={} phnum={}\n",
            e_entry, e_phoff, e_phentsize, e_phnum
        );
    }

    const PT_LOAD: u32 = 1;
    const PF_R: u32 = 4;
    const PF_W: u32 = 2;
    const PF_X: u32 = 1;

    let as_handle = ctx.address_spaces.create()?;

    // Map PT_LOAD segments
    //
    // We also capture the first RW PT_LOAD vaddr so we can derive a sensible
    // RISC-V `gp` when userspace does not provide one. Most RISC-V linkers
    // define `__global_pointer$` as `RW_SEGMENT_VADDR + 0x800`.
    let mut first_rw_vaddr: Option<usize> = None;
    for i in 0..e_phnum {
        let off = e_phoff.checked_add(i * e_phentsize).ok_or(AddressSpaceError::InvalidArgs)?;
        if off + 56 > elf.len() {
            return Err(AddressSpaceError::InvalidArgs.into());
        }
        let p_type = read_u32_le(elf, off)?;
        if p_type != PT_LOAD {
            continue;
        }
        let p_flags = read_u32_le(elf, off + 4)?;
        let p_offset = read_u64_le(elf, off + 8)? as usize;
        let p_vaddr = read_u64_le(elf, off + 16)? as usize;
        let p_filesz = read_u64_le(elf, off + 32)? as usize;
        let p_memsz = read_u64_le(elf, off + 40)? as usize;
        {
            use core::fmt::Write as _;
            let mut u = crate::uart::raw_writer();
            let first4 = if p_offset + 4 <= elf.len() {
                u32::from_le_bytes([
                    elf[p_offset],
                    elf[p_offset + 1],
                    elf[p_offset + 2],
                    elf[p_offset + 3],
                ])
            } else {
                0
            };
            let _ = write!(
                u,
                "[INFO exec] EXEC-ELF phdr load off=0x{:x} vaddr=0x{:x} filesz=0x{:x} memsz=0x{:x} first4=0x{:08x}\n",
                p_offset, p_vaddr, p_filesz, p_memsz, first4
            );
        }

        if p_memsz == 0 {
            continue;
        }
        if p_filesz > p_memsz {
            return Err(AddressSpaceError::InvalidArgs.into());
        }
        let end = p_offset.checked_add(p_filesz).ok_or(AddressSpaceError::InvalidArgs)?;
        if end > elf.len() {
            return Err(AddressSpaceError::InvalidArgs.into());
        }
        if (p_flags & PF_W != 0) && (p_flags & PF_X != 0) {
            return Err(AddressSpaceError::from(MapError::PermissionDenied).into());
        }

        // Capture the first RW load segment base for gp derivation (if needed).
        if first_rw_vaddr.is_none() && (p_flags & PF_W != 0) {
            first_rw_vaddr = Some(p_vaddr);
        }

        let page_off = p_vaddr & (PAGE_SIZE - 1);
        let aligned_vaddr = p_vaddr - page_off;
        let alloc_len =
            align_len(p_memsz.checked_add(page_off).ok_or(AddressSpaceError::InvalidArgs)?)
                .ok_or(AddressSpaceError::InvalidArgs)?;
        let (base, alloc_len) = VMO_POOL.lock().allocate(alloc_len)?;

        // Copy file payload
        if p_filesz != 0 {
            unsafe {
                ptr::copy_nonoverlapping(
                    elf.as_ptr().add(p_offset),
                    (base + page_off) as *mut u8,
                    p_filesz,
                );
            }
        }
        // BSS tail is already cleared by the full-allocation zeroing above.

        let mut flags = PageFlags::VALID | PageFlags::USER;
        if p_flags & PF_R != 0 {
            flags |= PageFlags::READ;
        }
        if p_flags & PF_W != 0 {
            flags |= PageFlags::WRITE;
        }
        if p_flags & PF_X != 0 {
            flags |= PageFlags::EXECUTE;
        }

        // Map pages
        let pages = alloc_len / PAGE_SIZE;
        for page in 0..pages {
            let va = aligned_vaddr
                .checked_add(page * PAGE_SIZE)
                .ok_or(AddressSpaceError::InvalidArgs)?;
            let pa = base.checked_add(page * PAGE_SIZE).ok_or(AddressSpaceError::InvalidArgs)?;
            ctx.address_spaces.map_page(as_handle, va, pa, flags)?;
        }
    }

    // CRITICAL (RISC-V): Ensure the I-cache sees freshly loaded user text.
    // The kernel just wrote executable bytes into memory. Without `fence.i`, the hart may execute
    // stale instructions at those virtual addresses (especially across AS switches).
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    unsafe {
        core::arch::asm!("fence.i", options(nostack));
    }

    // Choose a global pointer for the new task.
    //
    // - Prefer the userspace-provided value (init-lite service table extracts it from ELFs).
    // - Otherwise, derive it from the first RW PT_LOAD segment (common RISC-V convention).
    // - As a last resort, fall back to entry + 0x800 to avoid gp=0 crashes in tiny images.
    const RISCV_GP_BIAS: usize = 0x800;
    let derived_gp = first_rw_vaddr
        .and_then(|vaddr| vaddr.checked_add(RISCV_GP_BIAS))
        .or_else(|| e_entry.checked_add(RISCV_GP_BIAS))
        .unwrap_or(0);
    let gp = if typed.global_pointer != 0 { typed.global_pointer } else { derived_gp };
    {
        use core::fmt::Write as _;
        let mut u = crate::uart::raw_writer();
        let src = if typed.global_pointer != 0 {
            "arg"
        } else if first_rw_vaddr.is_some() {
            "rw+0x800"
        } else {
            "entry+0x800"
        };
        let _ = write!(u, "[INFO exec] EXEC-ELF gp=0x{:x} src={}\n", gp, src);
    }

    // Stack
    // Userspace init-lite expects its stack at 0x2000_0000; map downward from there.
    // Map head pages so the top-of-stack address (0x2000_0000) and a boundary page
    // above it are mapped, then seed SP two pages below the mapped top to avoid
    // touching the boundary. Leave a guard page above the boundary.
    let total_pages = typed
        .stack_pages
        .checked_add(11) // requested + 9 head pages + boundary page; guard stays unmapped
        .ok_or(AddressSpaceError::InvalidArgs)?;
    let stack_bytes = total_pages.checked_mul(PAGE_SIZE).ok_or(AddressSpaceError::InvalidArgs)?;
    let (stack_base, stack_len) = VMO_POOL.lock().allocate(stack_bytes)?;
    // Clear the freshly allocated stack to avoid stale data influencing user
    // register setup/prologue logic.
    unsafe {
        ptr::write_bytes(stack_base as *mut u8, 0, stack_len);
    }
    let user_stack_top: usize = 0x2000_0000;
    // Map through the former faulting address (boundary) and leave a guard above.
    let mapped_top =
        user_stack_top.checked_add(10 * PAGE_SIZE).ok_or(AddressSpaceError::InvalidArgs)?; // boundary page mapped; guard sits above
    let stack_bottom = mapped_top.checked_sub(stack_len).ok_or(AddressSpaceError::InvalidArgs)?;

    let stack_flags = PageFlags::VALID | PageFlags::USER | PageFlags::READ | PageFlags::WRITE;
    for page in 0..total_pages {
        let va =
            stack_bottom.checked_add(page * PAGE_SIZE).ok_or(AddressSpaceError::InvalidArgs)?;
        let pa = stack_base.checked_add(page * PAGE_SIZE).ok_or(AddressSpaceError::InvalidArgs)?;
        ctx.address_spaces.map_page(as_handle, va, pa, stack_flags)?;
    }
    log_info!(
        target: "exec",
        "STACK-MAP: va=0x{:x}-0x{:x} pa=0x{:x} pages={} sp=0x{:x}",
        stack_bottom,
        mapped_top.saturating_sub(1),
        stack_base,
        total_pages,
        user_stack_top
    );

    let sp_probe = mapped_top.checked_sub(2 * PAGE_SIZE).ok_or(AddressSpaceError::InvalidArgs)?;
    if let Ok(space) = ctx.address_spaces.get(as_handle) {
        let pt = space.page_table();
        let t_sp = pt.translate(sp_probe);
        let t_top_minus_1 = pt.translate(mapped_top.saturating_sub(1));
        let t_top = pt.translate(mapped_top);
        log_info!(
            target: "exec",
            "STACK-CHECK: base=0x{:x} top=0x{:x} top-1->0x{:x?} top->0x{:x?} sp->0x{:x?}",
            stack_bottom,
            mapped_top,
            t_top_minus_1,
            t_top,
            t_sp
        );
    }

    let entry_pc = VirtAddr::instr_aligned(e_entry).ok_or(AddressSpaceError::InvalidArgs)?;
    // Start SP one full page below the mapped top to stay clear of the boundary, 16-byte aligned.
    let stack_sp_raw = sp_probe & !0xf;
    let stack_sp = VirtAddr::new(stack_sp_raw).ok_or(AddressSpaceError::InvalidArgs)?;
    let bootstrap_slot = SlotIndex::decode(0);

    let parent = ctx.tasks.current_pid();
    let pid = ctx.tasks.spawn(
        parent,
        entry_pc,
        Some(stack_sp),
        Some(as_handle),
        gp,
        bootstrap_slot,
        ctx.scheduler,
        ctx.router,
        ctx.address_spaces,
    )?;

    // RFC-0004 Phase 1 diagnostics: store user guard metadata for trap attribution.
    if let Some(t) = ctx.tasks.task_mut(pid) {
        t.set_user_guard_info(task::UserGuardInfo {
            stack_guard_va: mapped_top,
            info_guard_va: None,
        });
    }

    Ok(pid.as_index())
}

/// Kernel-side exec loader v2: like [`sys_exec`] but also copies the provided service name bytes
/// into a per-service read-only mapping in the child address space (RFC-0004 provenance).
fn sys_exec_v2(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let typed = ExecV2ArgsTyped::decode(args)?;
    typed.check()?;

    // Capability gate: only tasks holding a SEND right in slot 0 (bootstrap cap) may exec.
    {
        let _ = ctx
            .tasks
            .current_caps_mut()
            .derive(0, Rights::SEND)
            .map_err(|_| Error::Capability(CapError::PermissionDenied))?;
    }

    // SAFETY: user slice validated by check; still best-effort.
    let elf = unsafe { slice::from_raw_parts(typed.elf_ptr as *const u8, typed.elf_len) };
    if elf.len() < 64 || &elf[0..4] != b"\x7FELF" {
        return Err(AddressSpaceError::InvalidArgs.into());
    }
    if elf[4] != 2 || elf[5] != 1 {
        return Err(AddressSpaceError::InvalidArgs.into()); // not ELF64/LE
    }

    let e_entry = read_u64_le(elf, 24)? as usize;
    let e_phoff = read_u64_le(elf, 32)? as usize;
    let e_phentsize = read_u16_le(elf, 54)? as usize;
    let e_phnum = read_u16_le(elf, 56)? as usize;
    if e_phoff >= elf.len() {
        return Err(AddressSpaceError::InvalidArgs.into());
    }

    const PT_LOAD: u32 = 1;
    const PF_R: u32 = 4;
    const PF_W: u32 = 2;
    const PF_X: u32 = 1;

    let as_handle = ctx.address_spaces.create()?;

    let mut first_rw_vaddr: Option<usize> = None;
    let mut max_end_va: usize = 0;

    // Track mapped PT_LOAD ranges to assert that any existing page-aligned gaps stay unmapped.
    // This is best-effort: we do NOT reject valid ELFs that have no gaps, but we do ensure we
    // never accidentally "inflate" a mapping into a gap.
    #[derive(Clone, Copy)]
    struct LoadRange {
        start: usize,
        end: usize, // page-aligned end
        writable: bool,
    }
    let mut load_ranges: alloc::vec::Vec<LoadRange> = alloc::vec::Vec::new();

    for i in 0..e_phnum {
        let off = e_phoff.checked_add(i * e_phentsize).ok_or(AddressSpaceError::InvalidArgs)?;
        if off + 56 > elf.len() {
            return Err(AddressSpaceError::InvalidArgs.into());
        }
        let p_type = read_u32_le(elf, off)?;
        if p_type != PT_LOAD {
            continue;
        }
        let p_flags = read_u32_le(elf, off + 4)?;
        let p_offset = read_u64_le(elf, off + 8)? as usize;
        let p_vaddr = read_u64_le(elf, off + 16)? as usize;
        let p_filesz = read_u64_le(elf, off + 32)? as usize;
        let p_memsz = read_u64_le(elf, off + 40)? as usize;

        if p_memsz == 0 {
            continue;
        }
        if p_filesz > p_memsz {
            return Err(AddressSpaceError::InvalidArgs.into());
        }
        let end = p_offset.checked_add(p_filesz).ok_or(AddressSpaceError::InvalidArgs)?;
        if end > elf.len() {
            return Err(AddressSpaceError::InvalidArgs.into());
        }
        if (p_flags & PF_W != 0) && (p_flags & PF_X != 0) {
            return Err(AddressSpaceError::from(MapError::PermissionDenied).into());
        }

        if first_rw_vaddr.is_none() && (p_flags & PF_W != 0) {
            first_rw_vaddr = Some(p_vaddr);
        }

        let page_off = p_vaddr & (PAGE_SIZE - 1);
        let aligned_vaddr = p_vaddr - page_off;
        let alloc_len =
            align_len(p_memsz.checked_add(page_off).ok_or(AddressSpaceError::InvalidArgs)?)
                .ok_or(AddressSpaceError::InvalidArgs)?;
        let (base, alloc_len) = VMO_POOL.lock().allocate(alloc_len)?;

        // Copy file payload (allocation is already zeroed by VmoPool::allocate).
        if p_filesz != 0 {
            unsafe {
                ptr::copy_nonoverlapping(
                    elf.as_ptr().add(p_offset),
                    (base + page_off) as *mut u8,
                    p_filesz,
                );
            }
        }

        let mut flags = PageFlags::VALID | PageFlags::USER;
        if p_flags & PF_R != 0 {
            flags |= PageFlags::READ;
        }
        if p_flags & PF_W != 0 {
            flags |= PageFlags::WRITE;
        }
        if p_flags & PF_X != 0 {
            flags |= PageFlags::EXECUTE;
        }

        let pages = alloc_len / PAGE_SIZE;
        for page in 0..pages {
            let va = aligned_vaddr
                .checked_add(page * PAGE_SIZE)
                .ok_or(AddressSpaceError::InvalidArgs)?;
            let pa = base.checked_add(page * PAGE_SIZE).ok_or(AddressSpaceError::InvalidArgs)?;
            ctx.address_spaces.map_page(as_handle, va, pa, flags)?;
        }

        let seg_end = aligned_vaddr.checked_add(alloc_len).ok_or(AddressSpaceError::InvalidArgs)?;
        max_end_va = core::cmp::max(max_end_va, seg_end);

        load_ranges.push(LoadRange {
            start: aligned_vaddr,
            end: seg_end,
            writable: (p_flags & PF_W) != 0,
        });
    }

    // Assert best-effort guard gaps between PT_LOAD mappings (if a gap exists).
    if let Ok(space) = ctx.address_spaces.get(as_handle) {
        load_ranges.sort_by_key(|r| r.start);
        for (idx, r) in load_ranges.iter().enumerate() {
            if !r.writable {
                continue;
            }
            let next_start = load_ranges.get(idx + 1).map(|n| n.start).unwrap_or(usize::MAX);
            if next_start >= r.end.saturating_add(PAGE_SIZE) && r.end < USER_VADDR_LIMIT {
                if space.page_table().lookup(r.end).is_some() {
                    panic!("exec_v2: PT_LOAD gap page unexpectedly mapped at 0x{:x}", r.end);
                }
            }
        }
    }

    // CRITICAL (RISC-V): Ensure the I-cache sees freshly loaded user text.
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    unsafe {
        core::arch::asm!("fence.i", options(nostack));
    }

    // Choose a global pointer for the new task (same policy as sys_exec).
    const RISCV_GP_BIAS: usize = 0x800;
    let derived_gp = first_rw_vaddr
        .and_then(|vaddr| vaddr.checked_add(RISCV_GP_BIAS))
        .or_else(|| e_entry.checked_add(RISCV_GP_BIAS))
        .unwrap_or(0);
    let gp = if typed.global_pointer != 0 { typed.global_pointer } else { derived_gp };

    // Stack (same policy as sys_exec).
    let total_pages = typed.stack_pages.checked_add(11).ok_or(AddressSpaceError::InvalidArgs)?;
    let stack_bytes = total_pages.checked_mul(PAGE_SIZE).ok_or(AddressSpaceError::InvalidArgs)?;
    let (stack_base, stack_len) = VMO_POOL.lock().allocate(stack_bytes)?;
    let user_stack_top: usize = 0x2000_0000;
    let mapped_top =
        user_stack_top.checked_add(10 * PAGE_SIZE).ok_or(AddressSpaceError::InvalidArgs)?;
    let stack_bottom = mapped_top.checked_sub(stack_len).ok_or(AddressSpaceError::InvalidArgs)?;
    let stack_flags = PageFlags::VALID | PageFlags::USER | PageFlags::READ | PageFlags::WRITE;
    for page in 0..total_pages {
        let va =
            stack_bottom.checked_add(page * PAGE_SIZE).ok_or(AddressSpaceError::InvalidArgs)?;
        let pa = stack_base.checked_add(page * PAGE_SIZE).ok_or(AddressSpaceError::InvalidArgs)?;
        ctx.address_spaces.map_page(as_handle, va, pa, stack_flags)?;
    }

    // Per-service metadata mapping (RO) + bootstrap info page (RO).
    //
    // We intentionally place these at stable addresses just above the mapped stack-top boundary:
    // - mapped_top is `user_stack_top + 10*PAGE_SIZE` (see stack policy)
    // - meta page: mapped_top + 1 page
    // - info page: mapped_top + 2 pages
    //
    // This keeps the contract simple for early userland while remaining provenance-safe.
    let meta_va = mapped_top.checked_add(PAGE_SIZE).ok_or(AddressSpaceError::InvalidArgs)?;
    let info_va = mapped_top.checked_add(2 * PAGE_SIZE).ok_or(AddressSpaceError::InvalidArgs)?;
    if info_va + PAGE_SIZE >= USER_VADDR_LIMIT {
        return Err(AddressSpaceError::InvalidArgs.into());
    }
    // Also ensure we don't overlap PT_LOAD segments (defensive).
    if max_end_va != 0 && (meta_va < max_end_va || info_va < max_end_va) {
        return Err(AddressSpaceError::InvalidArgs.into());
    }

    let (meta_pa, _meta_len) = VMO_POOL.lock().allocate(PAGE_SIZE)?;
    let mut service_id: u64 = 0;
    if typed.name_len != 0 {
        // SAFETY: checked in ExecV2ArgsTyped::check.
        let name_bytes =
            unsafe { slice::from_raw_parts(typed.name_ptr as *const u8, typed.name_len) };
        // Kernel-verified service identity token: FNV-1a 64 of the name bytes.
        // This is deterministic, does not allocate, and can be recomputed by userland for display.
        service_id = 0xcbf29ce484222325u64;
        for &b in name_bytes {
            service_id ^= b as u64;
            service_id = service_id.wrapping_mul(0x100000001b3u64);
        }
        unsafe {
            ptr::copy_nonoverlapping(name_bytes.as_ptr(), meta_pa as *mut u8, name_bytes.len());
            if name_bytes.len() < PAGE_SIZE {
                ptr::write((meta_pa + name_bytes.len()) as *mut u8, 0);
            }
        }
    }
    let meta_flags = PageFlags::VALID | PageFlags::USER | PageFlags::READ;
    ctx.address_spaces.map_page(as_handle, meta_va, meta_pa, meta_flags)?;

    // Bootstrap info page describing the metadata mapping (RO).
    let (info_pa, _info_len) = VMO_POOL.lock().allocate(PAGE_SIZE)?;
    {
        let info = crate::BootstrapInfo {
            version: 2,
            reserved: 0,
            meta_name_ptr: meta_va as u64,
            meta_name_len: typed.name_len as u32,
            reserved2: 0,
            service_id,
        };
        unsafe {
            ptr::copy_nonoverlapping(
                &info as *const _ as *const u8,
                info_pa as *mut u8,
                core::mem::size_of::<crate::BootstrapInfo>(),
            );
        }
    }
    let info_flags = PageFlags::VALID | PageFlags::USER | PageFlags::READ;
    ctx.address_spaces.map_page(as_handle, info_va, info_pa, info_flags)?;

    // Guard page above the bootstrap info page must remain unmapped.
    if let Ok(space) = ctx.address_spaces.get(as_handle) {
        let guard_va = info_va.checked_add(PAGE_SIZE).ok_or(AddressSpaceError::InvalidArgs)?;
        if guard_va < USER_VADDR_LIMIT && space.page_table().lookup(guard_va).is_some() {
            panic!("exec_v2: info guard page mapped at 0x{:x}", guard_va);
        }
    }

    // Proof marker: log the mapping entry (leaf flags must not include WRITE).
    if let Ok(space) = ctx.address_spaces.get(as_handle) {
        if let Some(entry) = space.page_table().lookup(meta_va) {
            use core::fmt::Write as _;
            let writable = (entry & PageFlags::WRITE.bits()) != 0;
            if writable {
                panic!("exec meta mapping writable");
            }
            let mut u = crate::uart::raw_writer();
            let _ = writeln!(
                u,
                "[INFO exec] EXEC-META: va=0x{:x} pa=0x{:x} entry=0x{:016x} name_len=0x{:x} info_va=0x{:x}",
                meta_va, meta_pa, entry, typed.name_len
                ,info_va
            );
        }
    }

    let entry_pc = VirtAddr::instr_aligned(e_entry).ok_or(AddressSpaceError::InvalidArgs)?;
    let sp_probe = mapped_top.checked_sub(2 * PAGE_SIZE).ok_or(AddressSpaceError::InvalidArgs)?;
    let stack_sp_raw = sp_probe & !0xf;
    let stack_sp = VirtAddr::new(stack_sp_raw).ok_or(AddressSpaceError::InvalidArgs)?;
    let bootstrap_slot = SlotIndex::decode(0);

    let parent = ctx.tasks.current_pid();
    let pid = ctx.tasks.spawn(
        parent,
        entry_pc,
        Some(stack_sp),
        Some(as_handle),
        gp,
        bootstrap_slot,
        ctx.scheduler,
        ctx.router,
        ctx.address_spaces,
    )?;

    // Bind identity to the spawned task (kernel-derived): used for IPC sender attribution.
    if let Some(t) = ctx.tasks.task_mut(pid) {
        t.set_service_id(service_id);
        // RFC-0004 Phase 1 diagnostics: store user guard metadata for trap attribution.
        let guard_va = info_va.checked_add(PAGE_SIZE).ok_or(AddressSpaceError::InvalidArgs)?;
        t.set_user_guard_info(task::UserGuardInfo {
            stack_guard_va: mapped_top,
            info_guard_va: Some(guard_va),
        });
    }

    // Future-facing: once IPC copy-out exists (RFC-0005), we will deliver a BootstrapMsg with
    // `flags::HAS_INFO_PAGE` and `argv_ptr=info_va`. For now, the info/meta pages are at stable
    // addresses and can be read directly by early services.

    Ok(pid.as_index())
}

/// Minimal debug UART write for userspace: writes one byte `a0` to UART.
/// Returns the byte written on success. This is best-effort and meant only
/// for early bring-up. It does not perform permission checks.
// CRITICAL: Debug only. No permission checks; avoid locks; do not expand scope.
fn sys_debug_putc(_ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let byte = (args.get(0) & 0xff) as u8;
    // Use the raw writer to avoid taking locks under scheduler paths.
    let mut u = crate::uart::raw_writer();
    use core::fmt::Write as _;
    let ch = [byte];
    let s = core::str::from_utf8(&ch).unwrap_or("");
    let _ = u.write_str(s);
    Ok(byte as usize)
}

// CRITICAL: ABI surface for userspace spawn. Keep Decode→Check→Execute and rights checks stable.
fn sys_spawn(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let typed = SpawnArgsTyped::decode(args)?;
    let sp_raw = typed.stack_sp.map(|v| v.raw()).unwrap_or(0);
    let as_raw = typed.as_handle.map(|h| h.to_raw()).unwrap_or(0);
    log_info!(
        target: "sys",
        "SPAWN: entry=0x{:x} sp=0x{:x} as={} slot={} gp=0x{:x}",
        typed.entry_pc.raw(),
        sp_raw,
        as_raw,
        typed.bootstrap_slot.0,
        typed.global_pointer
    );
    typed.check()?;

    let parent = ctx.tasks.current_pid();
    let pid = match ctx.tasks.spawn(
        parent,
        typed.entry_pc,
        typed.stack_sp,
        typed.as_handle,
        typed.global_pointer,
        typed.bootstrap_slot,
        ctx.scheduler,
        ctx.router,
        ctx.address_spaces,
    ) {
        Ok(pid) => pid,
        Err(err) => {
            let reason = crate::task::spawn_fail_reason(&err);
            ctx.tasks.set_last_spawn_fail_reason(parent, reason);
            return Err(err.into());
        }
    };

    Ok(pid.as_index())
}

fn sys_cap_transfer(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let typed = CapTransferArgsTyped::decode(args)?;
    let rights = typed.check()?;
    let parent = ctx.tasks.current_pid();
    #[cfg(feature = "ipc_trace_ring")]
    {
        if let Ok(parent_caps) =
            ctx.tasks.caps_of(parent).ok_or(Error::Transfer(task::TransferError::InvalidParent))
        {
            if let Ok(base) = parent_caps.get(typed.parent_slot.0) {
                if let CapabilityKind::Endpoint(id) = base.kind {
                    crate::ipc::trace::record_cap_xfer(
                        parent.as_raw(),
                        typed.child.as_raw(),
                        id,
                        rights.bits() as u16,
                    );
                }
            }
        }
    }
    // RFC-0005 Phase 2 (hardening): `Rights::MANAGE` is not transferable for endpoints.
    //
    // Exception: allow transferring MANAGE for the EndpointFactory capability, so init-lite can
    // hold endpoint-create authority without relying on PID/parentage checks.
    if rights.contains(Rights::MANAGE) {
        let parent_caps =
            ctx.tasks.caps_of(parent).ok_or(Error::Transfer(task::TransferError::InvalidParent))?;
        let base = parent_caps
            .get(typed.parent_slot.0)
            .map_err(|e| Error::Transfer(task::TransferError::Capability(e)))?;
        if base.kind != CapabilityKind::EndpointFactory {
            return Err(Error::Transfer(task::TransferError::Capability(
                CapError::PermissionDenied,
            )));
        }
    }

    // Phase-2 hardening (factory distribution): EndpointFactory is not a general transferable cap.
    // Until policyd-gated distribution exists, only bootstrap (PID 0) may transfer it into init-lite (PID 1).
    // This keeps endpoint-mint authority centralized in init-lite during bring-up.
    if let Ok(parent_caps) =
        ctx.tasks.caps_of(parent).ok_or(Error::Transfer(task::TransferError::InvalidParent))
    {
        // (This block is structured as "check then act" to keep denial deterministic.)
        if let Ok(base) = parent_caps.get(typed.parent_slot.0) {
            if base.kind == CapabilityKind::EndpointFactory {
                if !(parent == task::Pid::KERNEL && typed.child == task::Pid::from_raw(1)) {
                    return Err(Error::Transfer(task::TransferError::Capability(
                        CapError::PermissionDenied,
                    )));
                }
            }
        }
    }
    let slot = ctx.tasks.transfer_cap(parent, typed.child, typed.parent_slot.0, rights)?;
    Ok(slot)
}

fn sys_cap_transfer_to(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let typed = CapTransferToArgsTyped::decode(args)?;
    let rights = typed.check()?;
    let parent = ctx.tasks.current_pid();
    // RFC-0005 Phase 2 (hardening): `Rights::MANAGE` is not transferable for endpoints.
    //
    // Exception: allow transferring MANAGE for the EndpointFactory capability, so init-lite can
    // hold endpoint-create authority without relying on PID/parentage checks.
    if rights.contains(Rights::MANAGE) {
        let parent_caps =
            ctx.tasks.caps_of(parent).ok_or(Error::Transfer(task::TransferError::InvalidParent))?;
        let base = parent_caps
            .get(typed.parent_slot.0)
            .map_err(|e| Error::Transfer(task::TransferError::Capability(e)))?;
        if base.kind != CapabilityKind::EndpointFactory {
            return Err(Error::Transfer(task::TransferError::Capability(
                CapError::PermissionDenied,
            )));
        }
    }

    // Phase-2 hardening (factory distribution): EndpointFactory is not a general transferable cap.
    // Until policyd-gated distribution exists, only bootstrap (PID 0) may transfer it into init-lite (PID 1).
    // This keeps endpoint-mint authority centralized in init-lite during bring-up.
    if let Ok(parent_caps) =
        ctx.tasks.caps_of(parent).ok_or(Error::Transfer(task::TransferError::InvalidParent))
    {
        // (This block is structured as "check then act" to keep denial deterministic.)
        if let Ok(base) = parent_caps.get(typed.parent_slot.0) {
            if base.kind == CapabilityKind::EndpointFactory {
                if !(parent == task::Pid::KERNEL && typed.child == task::Pid::from_raw(1)) {
                    return Err(Error::Transfer(task::TransferError::Capability(
                        CapError::PermissionDenied,
                    )));
                }
            }
        }
    }
    ctx.tasks.transfer_cap_to_slot(
        parent,
        typed.child,
        typed.parent_slot.0,
        rights,
        typed.child_slot.0,
    )?;
    Ok(typed.child_slot.0)
}

const PROT_READ: u32 = 1 << 0;
const PROT_WRITE: u32 = 1 << 1;
const PROT_EXEC: u32 = 1 << 2;

const MAP_FLAG_USER: u32 = 1 << 0;
const USER_VADDR_LIMIT: usize = 0x8000_0000;
// Keep the kernel-managed user VMO arena away from the kernel stacks/data.
// The previous implicit choice (__bss_end aligned) overlapped the kernel stack
// pages (0x8048a000..), causing memcpy into the stack guard. Place it at a
// fixed high region in DRAM; virt QEMU gives us ample headroom.
const USER_VMO_ARENA_BASE: usize = 0x8100_0000;

static VMO_POOL: Mutex<VmoPool> = Mutex::new(VmoPool::new());

struct VmoPool {
    base: usize,
    next: usize,
    limit: usize,
}

impl VmoPool {
    const fn new() -> Self {
        Self { base: 0, next: 0, limit: 0 }
    }

    fn ensure_initialized(&mut self) {
        if self.base != 0 {
            return;
        }
        let start = align_up_addr(USER_VMO_ARENA_BASE);
        let limit = start.saturating_add(USER_VMO_ARENA_LEN);
        self.base = start;
        self.next = start;
        self.limit = limit;
    }

    fn allocate(&mut self, len: usize) -> Result<(usize, usize), Error> {
        self.ensure_initialized();
        if len == 0 {
            return Err(Error::Capability(CapError::PermissionDenied));
        }
        let aligned = align_len(len).ok_or(Error::Capability(CapError::PermissionDenied))?;
        let next =
            self.next.checked_add(aligned).ok_or(Error::Capability(CapError::PermissionDenied))?;
        if next > self.limit {
            return Err(Error::Capability(CapError::PermissionDenied));
        }
        let base = self.next;
        self.next = next;
        // RFC-0004 (loader + shared-page guard): ensure newly allocated pages never leak stale
        // contents (kernel pointers, prior service metadata, etc.) to user space.
        unsafe {
            ptr::write_bytes(base as *mut u8, 0, aligned);
        }
        Ok((base, aligned))
    }

    #[allow(dead_code)]
    fn contains(&self, addr: usize, len: usize) -> bool {
        if self.base == 0 || len == 0 {
            return false;
        }
        let end = match addr.checked_add(len) {
            Some(end) => end,
            None => return false,
        };
        addr >= self.base && end <= self.limit
    }
}

fn align_len(len: usize) -> Option<usize> {
    if len == 0 {
        Some(0)
    } else {
        len.checked_add(PAGE_SIZE - 1).map(|value| value & !(PAGE_SIZE - 1))
    }
}

fn align_up_addr(addr: usize) -> usize {
    let mask = PAGE_SIZE - 1;
    (addr + mask) & !mask
}

fn ensure_user_slice(ptr: usize, len: usize) -> Result<(), Error> {
    if len == 0 {
        return Ok(());
    }

    // Host tests run the kernel logic in-process; pointers won't fall under the Sv39 user VA range.
    // For tests, accept any non-overflowing slice address and rely on Rust/host memory safety.
    #[cfg(test)]
    {
        let _last = ptr.checked_add(len - 1).ok_or(AddressSpaceError::InvalidArgs)?;
        return Ok(());
    }

    // Non-test (real kernel): enforce Sv39 user VA range and reject null pointers.
    #[cfg(not(test))]
    {
        if ptr == 0 {
            return Err(AddressSpaceError::InvalidArgs.into());
        }
        if ptr >= USER_VADDR_LIMIT {
            return Err(AddressSpaceError::InvalidArgs.into());
        }
        let last = ptr.checked_add(len - 1).ok_or(AddressSpaceError::InvalidArgs)?;
        if last >= USER_VADDR_LIMIT {
            return Err(AddressSpaceError::InvalidArgs.into());
        }
        Ok(())
    }
}

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
    let span_bytes = pages.checked_mul(PAGE_SIZE).ok_or(AddressSpaceError::InvalidArgs)?;
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

    // RFC-0004: enforce W^X at the syscall boundary for user mappings.
    if flags.contains(PageFlags::WRITE) && flags.contains(PageFlags::EXECUTE) {
        return Err(AddressSpaceError::from(MapError::PermissionDenied).into());
    }

    #[cfg(feature = "debug_uart")]
    {
        use core::fmt::Write as _;
        let mut u = crate::uart::raw_writer();
        let _ = writeln!(
            u,
            "AS-MAP handle=0x{:x} slot=0x{:x} va=0x{:x} len=0x{:x} pages=0x{:x} base=0x{:x} prot=0x{:x} flags=0x{:x}",
            typed.handle.to_raw(),
            typed.vmo_slot.0,
            typed.va.raw(),
            typed.len.raw(),
            pages,
            base,
            typed.prot,
            flags.bits()
        );
    }

    #[cfg(feature = "debug_uart")]
    let mut logged_preview = false;

    for page in 0..pages {
        let page_va =
            typed.va.raw().checked_add(page * PAGE_SIZE).ok_or(AddressSpaceError::InvalidArgs)?;
        let page_pa = base.checked_add(page * PAGE_SIZE).ok_or(AddressSpaceError::InvalidArgs)?;
        ctx.address_spaces.map_page(typed.handle, page_va, page_pa, flags)?;
        #[cfg(feature = "debug_uart")]
        if !logged_preview {
            logged_preview = true;
            log_vmo_preview(typed.vmo_slot.0, page_pa, aligned_bytes, typed.prot);
        }
    }

    Ok(0)
}

#[cfg(feature = "debug_uart")]
fn log_vmo_preview(slot: usize, base: usize, len: u64, prot: u32) {
    use core::fmt::Write as _;

    let mut u = crate::uart::raw_writer();
    let preview_len = core::cmp::min(len, 16) as usize;

    let pool = VMO_POOL.lock();
    let in_pool = preview_len > 0 && pool.contains(base, preview_len);
    drop(pool);

    if !in_pool {
        let _ = write!(
            u,
            "VMO-PREVIEW skipped slot=0x{:x} base=0x{:x} len=0x{:x} prot=0x{:x}\n",
            slot, base, len, prot
        );
        return;
    }

    let mut buf = [0u8; 16];
    if preview_len > 0 {
        unsafe {
            core::ptr::copy_nonoverlapping(base as *const u8, buf.as_mut_ptr(), preview_len);
        }
    }
    let _ = write!(
        u,
        "VMO-PREVIEW slot=0x{:x} base=0x{:x} len=0x{:x} prot=0x{:x} bytes=",
        slot, base, len, prot
    );
    for byte in &buf[..preview_len] {
        let _ = write!(u, "{:02x}", byte);
    }
    let _ = u.write_str("\n");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        cap::{Capability, CapabilityKind, Rights},
        mm::AddressSpaceManager,
        syscall::{
            Args, SyscallTable, SYSCALL_CAP_TRANSFER, SYSCALL_IPC_RECV_V1, SYSCALL_RECV,
            SYSCALL_SEND, SYSCALL_SPAWN,
        },
        task::TaskTable,
        BootstrapMsg,
    };

    #[derive(Default)]
    struct MockTimer {
        now: core::cell::Cell<u64>,
    }

    impl MockTimer {
        fn set_now(&self, now: u64) {
            self.now.set(now);
        }
    }

    impl crate::hal::Timer for MockTimer {
        fn now(&self) -> u64 {
            self.now.get()
        }
        fn set_wakeup(&self, _deadline: u64) {}
    }

    #[test]
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
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
        as_manager.attach(kernel_as, task::Pid::KERNEL).unwrap();
        tasks.bootstrap_mut().address_space = Some(kernel_as);
        let timer = crate::hal::virt::VirtMachine::new();
        let mut ctx =
            Context::new(&mut scheduler, &mut tasks, &mut router, &mut as_manager, timer.timer());
        let mut table = SyscallTable::new();
        install_handlers(&mut table);

        table.dispatch(SYSCALL_SEND, &mut ctx, &Args::new([0, 1, 0, 0, 0, 0])).unwrap();
        let len = table.dispatch(SYSCALL_RECV, &mut ctx, &Args::new([0, 0, 0, 0, 0, 0])).unwrap();
        assert_eq!(len, 0);
        assert!(ctx.last_message().is_some());
    }

    #[test]
    fn ipc_v1_recv_deadline_times_out() {
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
        let mut router = ipc::Router::new(1);
        let mut as_manager = AddressSpaceManager::new();

        let timer = MockTimer::default();
        timer.set_now(100);

        let mut ctx =
            Context::new(&mut scheduler, &mut tasks, &mut router, &mut as_manager, &timer);

        let mut table = SyscallTable::new();
        install_handlers(&mut table);

        let mut hdr = crate::ipc::header::MessageHeader::new(0, 0, 0, 0, 0).to_le_bytes();
        let mut payload = [0u8; 8];
        let args = Args::new([
            0, // slot 0
            hdr.as_mut_ptr() as usize,
            payload.as_mut_ptr() as usize,
            payload.len(),
            0,   // sys_flags: blocking
            100, // deadline_ns: already expired
        ]);

        let err = table.dispatch(SYSCALL_IPC_RECV_V1, &mut ctx, &args).unwrap_err();
        assert_eq!(err, Error::Ipc(ipc::IpcError::TimedOut));
    }

    #[test]
    fn ipc_v1_send_queue_full_nonblock() {
        let mut scheduler = Scheduler::new();
        let mut tasks = TaskTable::new();
        let mut router = ipc::Router::new(0);
        let mut as_manager = AddressSpaceManager::new();
        let timer = MockTimer::default();

        // Create a depth-1 endpoint and grant SEND rights in slot 0.
        let endpoint = router.create_endpoint(1, None).unwrap();
        {
            let caps = tasks.bootstrap_mut().caps_mut();
            caps.set(
                0,
                Capability { kind: CapabilityKind::Endpoint(endpoint), rights: Rights::SEND },
            )
            .unwrap();
        }

        let mut ctx =
            Context::new(&mut scheduler, &mut tasks, &mut router, &mut as_manager, &timer);

        // Minimal valid header: len=0 (matches payload_len=0).
        let mut hdr = crate::ipc::header::MessageHeader::new(0, 0, 0, 0, 0).to_le_bytes();
        let args = Args::new([
            0,                         // cap slot
            hdr.as_mut_ptr() as usize, // header_ptr
            0,                         // payload_ptr (len=0)
            0,                         // payload_len
            IPC_SYS_NONBLOCK,          // sys_flags
            0,                         // deadline_ns
        ]);

        // First send fills the queue.
        assert!(sys_ipc_send_v1(&mut ctx, &args).is_ok());

        // Second send must fail with QueueFull (mapped to EAGAIN by trap.rs).
        match sys_ipc_send_v1(&mut ctx, &args) {
            Err(Error::Ipc(ipc::IpcError::QueueFull)) => {}
            other => panic!("expected QueueFull, got {:?}", other),
        }
    }

    #[test]
    fn ipc_v1_cap_move_blocking_deadline_times_out_and_preserves_cap() {
        let mut scheduler = Scheduler::new();
        let mut tasks = TaskTable::new();
        let mut router = ipc::Router::new(0);
        let mut as_manager = AddressSpaceManager::new();
        let timer = MockTimer::default();
        timer.set_now(100);

        // Endpoint depth=1, fill it so subsequent send hits QueueFull.
        let endpoint = router.create_endpoint(1, None).unwrap();
        let hdr0 = crate::ipc::header::MessageHeader::new(0, endpoint, 0, 0, 0);
        router
            .send(endpoint, crate::ipc::Message::new(hdr0, alloc::vec::Vec::new(), None))
            .unwrap();

        // Sender has SEND on endpoint in slot 0.
        {
            let caps = tasks.bootstrap_mut().caps_mut();
            caps.set(
                0,
                Capability { kind: CapabilityKind::Endpoint(endpoint), rights: Rights::SEND },
            )
            .unwrap();
            // Movable cap in slot 3 (no MANAGE). Use a live endpoint to avoid hardening rejection.
            let live_cap_ep = router.create_endpoint(1, None).unwrap();
            caps.set(
                3,
                Capability { kind: CapabilityKind::Endpoint(live_cap_ep), rights: Rights::SEND },
            )
            .unwrap();
        }

        let mut ctx =
            Context::new(&mut scheduler, &mut tasks, &mut router, &mut as_manager, &timer);

        // CAP_MOVE header: src=3, flags=CAP_MOVE, len=0.
        const IPC_HDR_CAP_MOVE: u16 = 1 << 0;
        let mut hdr =
            crate::ipc::header::MessageHeader::new(3, 0, 0, IPC_HDR_CAP_MOVE, 0).to_le_bytes();

        let args = Args::new([
            0,                         // endpoint cap slot
            hdr.as_mut_ptr() as usize, // header_ptr
            0,                         // payload_ptr (len=0)
            0,                         // payload_len
            0,                         // sys_flags (blocking)
            50,                        // deadline_ns (already expired vs now=100)
        ]);

        match sys_ipc_send_v1(&mut ctx, &args) {
            Err(Error::Ipc(ipc::IpcError::TimedOut)) => {}
            other => panic!("expected TimedOut, got {:?}", other),
        }

        // Cap must still be present in slot 3 (rollback guaranteed).
        assert!(ctx.tasks.current_caps_mut().get(3).is_ok());
    }

    #[test]
    fn ipc_v1_cap_move_recv_no_space_requeues_message() {
        let mut scheduler = Scheduler::new();
        let mut tasks = TaskTable::new();
        let mut router = ipc::Router::new(0);
        let mut as_manager = AddressSpaceManager::new();
        let timer = MockTimer::default();

        // Endpoint to receive from.
        let endpoint = router.create_endpoint(2, None).unwrap();

        // Fill all cap slots in the current task so allocation of the moved cap will fail.
        {
            let caps = tasks.bootstrap_mut().caps_mut();
            for i in 0..96 {
                caps.set(
                    i,
                    Capability { kind: CapabilityKind::Endpoint(i as u32), rights: Rights::SEND },
                )
                .unwrap();
            }
            // Slot 0 must be a RECV cap for the endpoint we will recv from.
            caps.set(
                0,
                Capability { kind: CapabilityKind::Endpoint(endpoint), rights: Rights::RECV },
            )
            .unwrap();
        }

        // Enqueue a message carrying a moved cap (some arbitrary endpoint cap).
        let hdr = crate::ipc::header::MessageHeader::new(0, endpoint, 0, 0, 0);
        router
            .send(
                endpoint,
                crate::ipc::Message::new(
                    hdr,
                    alloc::vec::Vec::new(),
                    Some(Capability { kind: CapabilityKind::Endpoint(999), rights: Rights::SEND }),
                ),
            )
            .unwrap();

        let mut ctx =
            Context::new(&mut scheduler, &mut tasks, &mut router, &mut as_manager, &timer);

        let mut out_hdr = [0u8; 16];
        let mut out_buf = [0u8; 8];
        let args = Args::new([
            0,                             // slot 0 (RECV cap)
            out_hdr.as_mut_ptr() as usize, // header_out_ptr
            out_buf.as_mut_ptr() as usize, // payload_out_ptr
            out_buf.len(),                 // payload_out_max
            0,                             // sys_flags (blocking ok)
            0,                             // deadline_ns
        ]);

        match sys_ipc_recv_v1(&mut ctx, &args) {
            Err(Error::Ipc(ipc::IpcError::NoSpace)) => {}
            other => panic!("expected NoSpace, got {:?}", other),
        }

        // Free one cap slot, then retry: recv should succeed and moved cap should be allocated.
        let _ = ctx.tasks.current_caps_mut().take(1);
        let n = sys_ipc_recv_v1(&mut ctx, &args).expect("recv after freeing slot");
        assert_eq!(n, 0);
        // The moved cap should have been allocated into some free slot (likely 1).
        assert!(ctx.tasks.current_caps_mut().get(1).is_ok());
    }

    #[test]
    fn cap_clone_returns_no_space_when_table_full() {
        let mut scheduler = Scheduler::new();
        let mut tasks = TaskTable::new();
        let mut router = ipc::Router::new(0);
        let mut as_manager = AddressSpaceManager::new();
        let timer = MockTimer::default();

        // Fill all cap slots (96). Ensure there is a valid cap at slot 0 to clone.
        {
            let caps = tasks.bootstrap_mut().caps_mut();
            for i in 0..96 {
                caps.set(
                    i,
                    Capability { kind: CapabilityKind::Endpoint(i as u32), rights: Rights::SEND },
                )
                .unwrap();
            }
        }

        let mut ctx =
            Context::new(&mut scheduler, &mut tasks, &mut router, &mut as_manager, &timer);

        match sys_cap_clone(&mut ctx, &Args::new([0, 0, 0, 0, 0, 0])) {
            Err(Error::Capability(CapError::NoSpace)) => {}
            other => panic!("expected CapError::NoSpace, got {:?}", other),
        }
    }

    #[test]
    fn cap_transfer_returns_no_space_when_child_table_full() {
        let mut scheduler = Scheduler::new();
        let mut tasks = TaskTable::new();
        let mut router = ipc::Router::new(0);
        let mut as_manager = AddressSpaceManager::new();
        let timer = MockTimer::default();

        // Selftest-only child creation avoids full address-space/spawn machinery in host tests.
        let child = tasks.selftest_create_dummy_task(task::Pid::KERNEL, &mut scheduler);

        // Cap to transfer lives in parent slot 3.
        {
            let caps = tasks.bootstrap_mut().caps_mut();
            caps.set(3, Capability { kind: CapabilityKind::Endpoint(123), rights: Rights::SEND })
                .unwrap();
        }

        // Fill the child's cap table fully so allocation fails.
        {
            let child_caps = tasks.task_mut(child).unwrap().caps_mut();
            for i in 0..96 {
                let _ = child_caps.set(
                    i,
                    Capability { kind: CapabilityKind::Endpoint(i as u32), rights: Rights::SEND },
                );
            }
        }

        let mut ctx =
            Context::new(&mut scheduler, &mut tasks, &mut router, &mut as_manager, &timer);

        // Args: child pid, parent slot, rights mask
        let args = Args::new([child.as_index(), 3, Rights::SEND.bits() as usize, 0, 0, 0]);
        match sys_cap_transfer(&mut ctx, &args) {
            Err(Error::Transfer(task::TransferError::Capability(CapError::NoSpace))) => {}
            other => panic!("expected TransferError::Capability(NoSpace), got {:?}", other),
        }
    }

    #[test]
    fn ipc_endpoint_create_quota_enforced() {
        let mut router = ipc::Router::new(0);
        // Keep this aligned with ipc::MAX_ENDPOINTS.
        for _ in 0..384 {
            router.create_endpoint(1, None).expect("create");
        }
        match router.create_endpoint(1, None) {
            Err(ipc::IpcError::NoSpace) => {}
            other => panic!("expected NoSpace, got {:?}", other),
        }
    }

    #[test]
    fn ipc_endpoint_create_owner_quota_enforced() {
        let mut router = ipc::Router::new(0);
        // Keep this aligned with ipc::MAX_ENDPOINTS_PER_OWNER.
        for _ in 0..96 {
            router.create_endpoint(1, Some(7)).expect("create");
        }
        match router.create_endpoint(1, Some(7)) {
            Err(ipc::IpcError::NoSpace) => {}
            other => panic!("expected NoSpace, got {:?}", other),
        }
        // Different owner should still be allowed (global limit not hit yet).
        router.create_endpoint(1, Some(8)).expect("create other owner");
    }

    #[test]
    fn ipc_v1_rights_denied_send() {
        let mut scheduler = Scheduler::new();
        let mut tasks = TaskTable::new();
        {
            let caps = tasks.bootstrap_mut().caps_mut();
            // Slot 0 has RECV only (no SEND).
            caps.set(0, Capability { kind: CapabilityKind::Endpoint(0), rights: Rights::RECV })
                .unwrap();
        }
        let mut router = ipc::Router::new(1);
        let mut as_manager = AddressSpaceManager::new();
        let timer = MockTimer::default();
        let mut ctx =
            Context::new(&mut scheduler, &mut tasks, &mut router, &mut as_manager, &timer);
        let mut table = SyscallTable::new();
        install_handlers(&mut table);

        let hdr = crate::ipc::header::MessageHeader::new(0, 0, 1, 0, 0).to_le_bytes();
        let args = Args::new([
            0,                     // slot 0
            hdr.as_ptr() as usize, // header_ptr
            0,                     // payload_ptr (len=0)
            0,                     // payload_len
            IPC_SYS_NONBLOCK,      // sys_flags
            0,                     // deadline_ns
        ]);
        let err = table.dispatch(SYSCALL_IPC_SEND_V1, &mut ctx, &args).unwrap_err();
        assert_eq!(err, Error::Capability(CapError::PermissionDenied));
    }

    #[test]
    fn ipc_v1_rights_denied_recv() {
        let mut scheduler = Scheduler::new();
        let mut tasks = TaskTable::new();
        {
            let caps = tasks.bootstrap_mut().caps_mut();
            // Slot 0 has SEND only (no RECV).
            caps.set(0, Capability { kind: CapabilityKind::Endpoint(0), rights: Rights::SEND })
                .unwrap();
        }
        let mut router = ipc::Router::new(1);
        let mut as_manager = AddressSpaceManager::new();
        let timer = MockTimer::default();
        let mut ctx =
            Context::new(&mut scheduler, &mut tasks, &mut router, &mut as_manager, &timer);
        let mut table = SyscallTable::new();
        install_handlers(&mut table);

        let mut hdr_out = [0u8; 16];
        let mut payload_out = [0u8; 8];
        let args = Args::new([
            0, // slot 0
            hdr_out.as_mut_ptr() as usize,
            payload_out.as_mut_ptr() as usize,
            payload_out.len(),
            IPC_SYS_NONBLOCK, // sys_flags
            0,                // deadline_ns
        ]);
        let err = table.dispatch(SYSCALL_IPC_RECV_V1, &mut ctx, &args).unwrap_err();
        assert_eq!(err, Error::Capability(CapError::PermissionDenied));
    }

    #[test]
    fn ipc_v1_cap_move_roundtrip_same_task() {
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
            // Slot 3: moveable VMO cap.
            caps.set(
                3,
                Capability {
                    kind: CapabilityKind::Vmo { base: 0x9000_0000, len: PAGE_SIZE },
                    rights: Rights::MAP,
                },
            )
            .unwrap();
        }
        let mut router = ipc::Router::new(1);
        let mut as_manager = AddressSpaceManager::new();
        let timer = MockTimer::default();
        let mut ctx =
            Context::new(&mut scheduler, &mut tasks, &mut router, &mut as_manager, &timer);
        let mut table = SyscallTable::new();
        install_handlers(&mut table);

        let payload = [1u8, 2, 3, 4];
        const IPC_HDR_CAP_MOVE: u16 = 1 << 0;
        let mut send_hdr = crate::ipc::header::MessageHeader::new(
            3, // cap slot to move (interpreted only when CAP_MOVE is set)
            0,
            0x55,
            IPC_HDR_CAP_MOVE,
            payload.len() as u32,
        )
        .to_le_bytes();

        // Send with CAP_MOVE (nonblocking).
        let send_args = Args::new([
            0,                              // endpoint cap slot
            send_hdr.as_mut_ptr() as usize, // header ptr
            payload.as_ptr() as usize,      // payload ptr
            payload.len(),                  // payload len
            IPC_SYS_NONBLOCK as usize,      // sys_flags
            0,                              // deadline
        ]);
        table.dispatch(SYSCALL_IPC_SEND_V1, &mut ctx, &send_args).unwrap();

        // Sender cap slot must be empty after send.
        assert_eq!(ctx.tasks.bootstrap_mut().caps_mut().get(3).unwrap_err(), CapError::InvalidSlot);

        // Receive and verify the cap was allocated back into slot 3.
        let mut out_hdr = [0u8; 16];
        let mut out_payload = [0u8; 8];
        let recv_args = Args::new([
            0,
            out_hdr.as_mut_ptr() as usize,
            out_payload.as_mut_ptr() as usize,
            out_payload.len(),
            IPC_SYS_NONBLOCK as usize,
            0,
        ]);
        let n = table.dispatch(SYSCALL_IPC_RECV_V1, &mut ctx, &recv_args).unwrap();
        assert_eq!(n, payload.len());
        assert_eq!(&out_payload[..payload.len()], &payload);

        let hdr = crate::ipc::header::MessageHeader::from_le_bytes(out_hdr);
        let moved_slot = hdr.src as usize;
        let cap = ctx.tasks.bootstrap_mut().caps_mut().get(moved_slot).unwrap();
        assert!(matches!(cap.kind, CapabilityKind::Vmo { .. }));
        // Original slot remains empty (we moved *out* of slot 3).
        assert_eq!(ctx.tasks.bootstrap_mut().caps_mut().get(3).unwrap_err(), CapError::InvalidSlot);
    }

    #[test]
    fn ipc_v1_nonblocking_queue_empty() {
        let mut scheduler = Scheduler::new();
        let mut tasks = TaskTable::new();
        {
            let caps = tasks.bootstrap_mut().caps_mut();
            caps.set(0, Capability { kind: CapabilityKind::Endpoint(0), rights: Rights::RECV })
                .unwrap();
        }
        let mut router = ipc::Router::new(1);
        let mut as_manager = AddressSpaceManager::new();
        let timer = MockTimer::default();
        let mut ctx =
            Context::new(&mut scheduler, &mut tasks, &mut router, &mut as_manager, &timer);
        let mut table = SyscallTable::new();
        install_handlers(&mut table);

        let mut hdr_out = [0u8; 16];
        let mut payload_out = [0u8; 8];
        let args = Args::new([
            0,
            hdr_out.as_mut_ptr() as usize,
            payload_out.as_mut_ptr() as usize,
            payload_out.len(),
            IPC_SYS_NONBLOCK,
            0,
        ]);
        let err = table.dispatch(SYSCALL_IPC_RECV_V1, &mut ctx, &args).unwrap_err();
        assert_eq!(err, Error::Ipc(ipc::IpcError::QueueEmpty));
    }

    #[test]
    fn ipc_v1_nonblocking_queue_full() {
        let mut scheduler = Scheduler::new();
        let mut tasks = TaskTable::new();
        {
            let caps = tasks.bootstrap_mut().caps_mut();
            caps.set(0, Capability { kind: CapabilityKind::Endpoint(0), rights: Rights::SEND })
                .unwrap();
        }
        let mut router = ipc::Router::new(1);
        let mut as_manager = AddressSpaceManager::new();
        let timer = MockTimer::default();
        let mut ctx =
            Context::new(&mut scheduler, &mut tasks, &mut router, &mut as_manager, &timer);
        let mut table = SyscallTable::new();
        install_handlers(&mut table);

        let hdr = crate::ipc::header::MessageHeader::new(0, 0, 1, 0, 0).to_le_bytes();
        let args = Args::new([0, hdr.as_ptr() as usize, 0, 0, IPC_SYS_NONBLOCK, 0]);

        // Router endpoint depth is 8. Fill it.
        for _ in 0..8 {
            table.dispatch(SYSCALL_IPC_SEND_V1, &mut ctx, &args).expect("send should fit");
        }
        let err = table.dispatch(SYSCALL_IPC_SEND_V1, &mut ctx, &args).unwrap_err();
        assert_eq!(err, Error::Ipc(ipc::IpcError::QueueFull));
    }

    #[test]
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
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
        as_manager.attach(kernel_as, task::Pid::KERNEL).unwrap();
        tasks.bootstrap_mut().address_space = Some(kernel_as);
        let timer = crate::hal::virt::VirtMachine::new();
        let mut ctx =
            Context::new(&mut scheduler, &mut tasks, &mut router, &mut as_manager, timer.timer());
        let mut table = SyscallTable::new();
        install_handlers(&mut table);

        let child = task::Pid::from_raw(
            table.dispatch(SYSCALL_SPAWN, &mut ctx, &Args::new([0x1000, 0, 0, 0, 0, 0])).unwrap()
                as u32,
        );
        assert_eq!(child, task::Pid::from_raw(1));
        let msg = ctx.router.recv(0).unwrap();
        assert_eq!(msg.payload.len(), core::mem::size_of::<BootstrapMsg>());

        let slot = table
            .dispatch(
                SYSCALL_CAP_TRANSFER,
                &mut ctx,
                &Args::new([child.as_index(), 0, Rights::SEND.bits() as usize, 0, 0, 0]),
            )
            .unwrap();
        assert_ne!(slot, 0);
        let cap = ctx.tasks.caps_of(child).unwrap().get(slot).unwrap();
        assert_eq!(cap.rights, Rights::SEND);

        // Subset mask (2): transfer RECV only.
        let slot2 = table
            .dispatch(
                SYSCALL_CAP_TRANSFER,
                &mut ctx,
                &Args::new([child.as_index(), 0, Rights::RECV.bits() as usize, 0, 0, 0]),
            )
            .unwrap();
        let cap2 = ctx.tasks.caps_of(child).unwrap().get(slot2).unwrap();
        assert_eq!(cap2.rights, Rights::RECV);

        // Superset rejection: MAP is not allowed by the parent cap in slot 0.
        let err = table
            .dispatch(
                SYSCALL_CAP_TRANSFER,
                &mut ctx,
                &Args::new([child.as_index(), 0, Rights::MAP.bits() as usize, 0, 0, 0]),
            )
            .unwrap_err();
        assert_eq!(
            err,
            Error::Transfer(task::TransferError::Capability(CapError::PermissionDenied))
        );
    }

    #[test]
    fn cap_transfer_rejects_invalid_rights_mask() {
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
        let mut router = ipc::Router::new(1);
        let mut as_manager = AddressSpaceManager::new();
        let timer = MockTimer::default();
        let mut ctx =
            Context::new(&mut scheduler, &mut tasks, &mut router, &mut as_manager, &timer);
        let mut table = SyscallTable::new();
        install_handlers(&mut table);

        // rights_bits contains an unknown bit (bit 31); decode must fail deterministically.
        let invalid_bits = 1u32 << 31;
        let err = table
            .dispatch(
                SYSCALL_CAP_TRANSFER,
                &mut ctx,
                &Args::new([1, 0, invalid_bits as usize, 0, 0, 0]),
            )
            .unwrap_err();
        assert_eq!(
            err,
            Error::Transfer(task::TransferError::Capability(CapError::PermissionDenied))
        );
    }

    #[test]
    fn cap_close_is_local_drop_only() {
        let mut scheduler = Scheduler::new();
        let mut tasks = TaskTable::new();
        // Seed caps before building a Context (which mutably borrows the task table).
        {
            let caps = tasks.bootstrap_mut().caps_mut();
            caps.set(
                0,
                Capability {
                    kind: CapabilityKind::Endpoint(0),
                    rights: Rights::SEND | Rights::RECV | Rights::MANAGE,
                },
            )
            .unwrap();
            // Also keep a non-MANAGE sender reference to the same endpoint so we can observe
            // "global close" (router returns NoSuchEndpoint).
            caps.set(1, Capability { kind: CapabilityKind::Endpoint(0), rights: Rights::SEND })
                .unwrap();
        }

        let mut router = ipc::Router::new(1);
        let mut as_manager = AddressSpaceManager::new();
        let timer = MockTimer::default();
        let mut ctx =
            Context::new(&mut scheduler, &mut tasks, &mut router, &mut as_manager, &timer);
        let mut table = SyscallTable::new();
        install_handlers(&mut table);

        // Close the cap: local drop only (endpoint stays alive).
        table
            .dispatch(crate::syscall::SYSCALL_CAP_CLOSE, &mut ctx, &Args::new([0, 0, 0, 0, 0, 0]))
            .unwrap();

        // Endpoint is still alive, so sending on the other cap should succeed.
        let hdr = crate::ipc::header::MessageHeader::new(0, 0, 1, 0, 0).to_le_bytes();
        let send_args = Args::new([
            1,                     // slot 1 (SEND)
            hdr.as_ptr() as usize, // header_ptr
            0,
            0,
            IPC_SYS_NONBLOCK,
            0,
        ]);
        table.dispatch(SYSCALL_IPC_SEND_V1, &mut ctx, &send_args).unwrap();
    }

    #[test]
    fn endpoint_close_denied_without_manage() {
        let mut scheduler = Scheduler::new();
        let mut tasks = TaskTable::new();
        // Seed caps before building a Context (which mutably borrows the task table).
        {
            let caps = tasks.bootstrap_mut().caps_mut();
            // Slot 0: endpoint cap WITHOUT MANAGE (attempting endpoint_close should be denied).
            caps.set(
                0,
                Capability {
                    kind: CapabilityKind::Endpoint(0),
                    rights: Rights::SEND | Rights::RECV,
                },
            )
            .unwrap();
            // Slot 1: a sender ref so we can verify the endpoint is still alive after the denied close.
            caps.set(1, Capability { kind: CapabilityKind::Endpoint(0), rights: Rights::SEND })
                .unwrap();
        }

        let mut router = ipc::Router::new(1);
        let mut as_manager = AddressSpaceManager::new();
        let timer = MockTimer::default();
        let mut ctx =
            Context::new(&mut scheduler, &mut tasks, &mut router, &mut as_manager, &timer);
        let mut table = SyscallTable::new();
        install_handlers(&mut table);

        let err = table
            .dispatch(
                crate::syscall::SYSCALL_IPC_ENDPOINT_CLOSE,
                &mut ctx,
                &Args::new([0, 0, 0, 0, 0, 0]),
            )
            .unwrap_err();
        assert_eq!(err, Error::Capability(CapError::PermissionDenied));

        // Endpoint should still be alive, so send should succeed.
        let hdr = crate::ipc::header::MessageHeader::new(0, 0, 1, 0, 0).to_le_bytes();
        let send_args = Args::new([
            1,                     // slot 1 (SEND)
            hdr.as_ptr() as usize, // header_ptr
            0,
            0,
            IPC_SYS_NONBLOCK,
            0,
        ]);
        table.dispatch(SYSCALL_IPC_SEND_V1, &mut ctx, &send_args).unwrap();
    }

    #[test]
    fn endpoint_close_allowed_with_manage_closes_globally() {
        let mut scheduler = Scheduler::new();
        let mut tasks = TaskTable::new();
        {
            let caps = tasks.bootstrap_mut().caps_mut();
            // Slot 0: MANAGE authority to close.
            caps.set(
                0,
                Capability {
                    kind: CapabilityKind::Endpoint(0),
                    rights: Rights::SEND | Rights::RECV | Rights::MANAGE,
                },
            )
            .unwrap();
            // Slot 1: non-MANAGE sender reference used to observe global close.
            caps.set(1, Capability { kind: CapabilityKind::Endpoint(0), rights: Rights::SEND })
                .unwrap();
        }

        let mut router = ipc::Router::new(1);
        let mut as_manager = AddressSpaceManager::new();
        let timer = MockTimer::default();
        let mut ctx =
            Context::new(&mut scheduler, &mut tasks, &mut router, &mut as_manager, &timer);
        let mut table = SyscallTable::new();
        install_handlers(&mut table);

        table
            .dispatch(
                crate::syscall::SYSCALL_IPC_ENDPOINT_CLOSE,
                &mut ctx,
                &Args::new([0, 0, 0, 0, 0, 0]),
            )
            .unwrap();

        let hdr = crate::ipc::header::MessageHeader::new(0, 0, 1, 0, 0).to_le_bytes();
        let send_args = Args::new([
            1,                     // slot 1 (still a valid cap, but endpoint should be closed)
            hdr.as_ptr() as usize, // header_ptr
            0,
            0,
            IPC_SYS_NONBLOCK,
            0,
        ]);
        let err = table.dispatch(SYSCALL_IPC_SEND_V1, &mut ctx, &send_args).unwrap_err();
        assert_eq!(err, Error::Ipc(ipc::IpcError::NoSuchEndpoint));
    }

    #[test]
    fn cap_transfer_rejects_manage_right() {
        let mut scheduler = Scheduler::new();
        let mut tasks = TaskTable::new();
        {
            let caps = tasks.bootstrap_mut().caps_mut();
            caps.set(
                0,
                Capability {
                    kind: CapabilityKind::Endpoint(0),
                    rights: Rights::SEND | Rights::RECV | Rights::MANAGE,
                },
            )
            .unwrap();
        }

        let mut router = ipc::Router::new(1);
        let mut as_manager = AddressSpaceManager::new();
        let timer = MockTimer::default();
        let mut ctx =
            Context::new(&mut scheduler, &mut tasks, &mut router, &mut as_manager, &timer);
        let mut table = SyscallTable::new();
        install_handlers(&mut table);

        // Attempt to transfer MANAGE: should be denied (Phase-2 hardening).
        let err = table
            .dispatch(
                SYSCALL_CAP_TRANSFER,
                &mut ctx,
                // Child PID does not need to exist; the rights check rejects first.
                &Args::new([1, 0, Rights::MANAGE.bits() as usize, 0, 0, 0]),
            )
            .unwrap_err();
        assert_eq!(
            err,
            Error::Transfer(task::TransferError::Capability(CapError::PermissionDenied))
        );
    }

    #[test]
    fn cap_transfer_allows_manage_for_endpoint_factory() {
        let mut scheduler = Scheduler::new();
        let mut tasks = TaskTable::new();
        {
            let caps = tasks.bootstrap_mut().caps_mut();
            caps.set(
                0,
                Capability { kind: CapabilityKind::EndpointFactory, rights: Rights::MANAGE },
            )
            .unwrap();
        }
        let mut router = ipc::Router::new(1);
        let mut as_manager = AddressSpaceManager::new();
        let timer = MockTimer::default();
        let mut ctx =
            Context::new(&mut scheduler, &mut tasks, &mut router, &mut as_manager, &timer);
        let mut table = SyscallTable::new();
        install_handlers(&mut table);

        let err = table
            .dispatch(
                SYSCALL_CAP_TRANSFER,
                &mut ctx,
                &Args::new([1, 0, Rights::MANAGE.bits() as usize, 0, 0, 0]),
            )
            .unwrap_err();
        // Fails because child doesn't exist, *not* because MANAGE is rejected for EndpointFactory.
        assert_eq!(err, Error::Transfer(task::TransferError::InvalidChild));
    }

    #[test]
    fn cap_clone_duplicates_local_cap() {
        let mut scheduler = Scheduler::new();
        let mut tasks = TaskTable::new();
        {
            let caps = tasks.bootstrap_mut().caps_mut();
            caps.set(
                3,
                Capability {
                    kind: CapabilityKind::Vmo { base: 0x9000_0000, len: PAGE_SIZE },
                    rights: Rights::MAP,
                },
            )
            .unwrap();
        }
        let mut router = ipc::Router::new(1);
        let mut as_manager = AddressSpaceManager::new();
        let timer = MockTimer::default();
        let mut ctx =
            Context::new(&mut scheduler, &mut tasks, &mut router, &mut as_manager, &timer);
        let mut table = SyscallTable::new();
        install_handlers(&mut table);

        let new_slot = table
            .dispatch(crate::syscall::SYSCALL_CAP_CLONE, &mut ctx, &Args::new([3, 0, 0, 0, 0, 0]))
            .unwrap();
        assert_ne!(new_slot, 3);
        assert!(matches!(
            ctx.tasks.bootstrap_mut().caps_mut().get(3).unwrap().kind,
            CapabilityKind::Vmo { .. }
        ));
        assert!(matches!(
            ctx.tasks.bootstrap_mut().caps_mut().get(new_slot as usize).unwrap().kind,
            CapabilityKind::Vmo { .. }
        ));
    }

    #[test]
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    fn cap_transfer_rejects_endpoint_factory_distribution_from_non_bootstrap() {
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
            // Bootstrap holds the endpoint factory in slot 2.
            caps.set(
                2,
                Capability { kind: CapabilityKind::EndpointFactory, rights: Rights::MANAGE },
            )
            .unwrap();
        }
        let mut router = ipc::Router::new(8);
        let mut as_manager = AddressSpaceManager::new();
        let timer = MockTimer::default();
        let mut ctx =
            Context::new(&mut scheduler, &mut tasks, &mut router, &mut as_manager, &timer);
        let mut table = SyscallTable::new();
        install_handlers(&mut table);

        // Spawn pid 1 (init-lite stand-in).
        let pid1 = table
            .dispatch(SYSCALL_SPAWN, &mut ctx, &Args::new([0x1000, 0, 0, 0, 0, 0]))
            .map(|pid| task::Pid::from_raw(pid as u32))
            .unwrap();
        assert_eq!(pid1, task::Pid::from_raw(1));

        // PID 0 -> PID 1 transfer is allowed (bootstrap distribution).
        let factory_slot_pid1 = table
            .dispatch(
                SYSCALL_CAP_TRANSFER,
                &mut ctx,
                &Args::new([pid1.as_index(), 2, Rights::MANAGE.bits() as usize, 0, 0, 0]),
            )
            .unwrap();
        assert_eq!(factory_slot_pid1, 1);

        // Switch to pid 1 and spawn its child pid 2.
        ctx.tasks.set_current(pid1);
        let pid2 = table
            .dispatch(SYSCALL_SPAWN, &mut ctx, &Args::new([0x1000, 0, 0, 0, 0, 0]))
            .map(|pid| task::Pid::from_raw(pid as u32))
            .unwrap();
        assert_eq!(pid2, task::Pid::from_raw(2));

        // PID 1 must NOT be able to distribute EndpointFactory further.
        let err = table
            .dispatch(
                SYSCALL_CAP_TRANSFER,
                &mut ctx,
                &Args::new([
                    pid2.as_index(),
                    factory_slot_pid1,
                    Rights::MANAGE.bits() as usize,
                    0,
                    0,
                    0,
                ]),
            )
            .unwrap_err();
        assert_eq!(
            err,
            Error::Transfer(task::TransferError::Capability(CapError::PermissionDenied))
        );
    }

    #[test]
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    fn ipc_endpoint_create_for_denies_non_parent_owner() {
        let mut scheduler = Scheduler::new();
        let mut tasks = TaskTable::new();
        {
            let caps = tasks.bootstrap_mut().caps_mut();
            // Seed bootstrap endpoint for spawn syscall.
            caps.set(
                0,
                Capability {
                    kind: CapabilityKind::Endpoint(0),
                    rights: Rights::SEND | Rights::RECV,
                },
            )
            .unwrap();
            // Seed endpoint factory in bootstrap (slot 2) so it can be transferred to pid1.
            caps.set(
                2,
                Capability { kind: CapabilityKind::EndpointFactory, rights: Rights::MANAGE },
            )
            .unwrap();
        }
        let mut router = ipc::Router::new(8);
        let mut as_manager = AddressSpaceManager::new();
        let timer = MockTimer::default();
        let mut ctx =
            Context::new(&mut scheduler, &mut tasks, &mut router, &mut as_manager, &timer);
        let mut table = SyscallTable::new();
        install_handlers(&mut table);

        // Spawn pid 1 (init-lite stand-in) and switch to it.
        let pid1 = table
            .dispatch(SYSCALL_SPAWN, &mut ctx, &Args::new([0x1000, 0, 0, 0, 0, 0]))
            .map(|pid| task::Pid::from_raw(pid as u32))
            .unwrap();
        assert_eq!(pid1, task::Pid::from_raw(1));
        ctx.tasks.set_current(pid1);

        // Transfer EndpointFactory into pid1 slot 1.
        let factory_slot = table
            .dispatch(
                SYSCALL_CAP_TRANSFER,
                &mut ctx,
                &Args::new([pid1.as_index(), 2, Rights::MANAGE.bits() as usize, 0, 0, 0]),
            )
            .unwrap();
        assert_eq!(factory_slot, 1);

        // Spawn pid 2 (child of pid1) and pid 3 (also child of pid1).
        let pid2 = table
            .dispatch(SYSCALL_SPAWN, &mut ctx, &Args::new([0x1000, 0, 0, 0, 0, 0]))
            .map(|pid| task::Pid::from_raw(pid as u32))
            .unwrap();
        let pid3 = table
            .dispatch(SYSCALL_SPAWN, &mut ctx, &Args::new([0x1000, 0, 0, 0, 0, 0]))
            .map(|pid| task::Pid::from_raw(pid as u32))
            .unwrap();
        assert_eq!(pid2, task::Pid::from_raw(2));
        assert_eq!(pid3, task::Pid::from_raw(3));

        // Switch to pid2 and attempt to create an endpoint owned by pid3.
        // Denied because pid2 is not the parent of pid3 (both are siblings under pid1).
        ctx.tasks.set_current(pid2);
        // Give pid2 the factory (init-lite would normally hold it; for test we transfer to pid2).
        let factory_slot_pid2 = table
            .dispatch(
                SYSCALL_CAP_TRANSFER,
                &mut ctx,
                &Args::new([pid2.as_index(), factory_slot, Rights::MANAGE.bits() as usize, 0, 0, 0]),
            )
            .unwrap();
        assert_ne!(factory_slot_pid2, 0);

        let err = table
            .dispatch(
                crate::syscall::SYSCALL_IPC_ENDPOINT_CREATE_FOR,
                &mut ctx,
                &Args::new([factory_slot_pid2, pid3.as_index(), 8, 0, 0, 0]),
            )
            .unwrap_err();
        assert_eq!(err, Error::Capability(CapError::PermissionDenied));
    }

    #[test]
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    fn endpoint_create_is_init_lite_only() {
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
        let mut router = ipc::Router::new(1);
        let mut as_manager = AddressSpaceManager::new();
        let timer = MockTimer::default();
        let mut ctx =
            Context::new(&mut scheduler, &mut tasks, &mut router, &mut as_manager, &timer);
        let mut table = SyscallTable::new();
        install_handlers(&mut table);

        // Spawn pid 1 (init-lite stand-in) and switch to it.
        let pid1 = table
            .dispatch(SYSCALL_SPAWN, &mut ctx, &Args::new([0x1000, 0, 0, 0, 0, 0]))
            .map(|pid| task::Pid::from_raw(pid as u32))
            .unwrap();
        assert_eq!(pid1, task::Pid::from_raw(1));
        ctx.tasks.set_current(pid1);

        // pid 1 may create endpoints.
        let slot = table
            .dispatch(SYSCALL_IPC_ENDPOINT_CREATE, &mut ctx, &Args::new([8, 0, 0, 0, 0, 0]))
            .unwrap();
        assert_ne!(slot, 0);

        // Spawn pid 2 (regular service stand-in, child of init-lite) and switch to it.
        let pid2 = table
            .dispatch(SYSCALL_SPAWN, &mut ctx, &Args::new([0x1000, 0, 0, 0, 0, 0]))
            .map(|pid| task::Pid::from_raw(pid as u32))
            .unwrap();
        assert_eq!(pid2, task::Pid::from_raw(2));
        ctx.tasks.set_current(pid2);

        // pid 2 is userspace too, but must be denied by the endpoint-factory gate.
        let err = table
            .dispatch(SYSCALL_IPC_ENDPOINT_CREATE, &mut ctx, &Args::new([8, 0, 0, 0, 0, 0]))
            .unwrap_err();
        assert_eq!(err, Error::Capability(CapError::PermissionDenied));
    }

    // ==========================================================================
    // MMIO capability negative tests (security floor for device access model)
    // ==========================================================================

    /// Test that mapping without a capability in the slot is rejected.
    /// Security invariant: MMIO access must be capability-gated.
    #[test]
    fn test_reject_mmio_no_cap() {
        use super::SYSCALL_MMIO_MAP;

        let mut scheduler = Scheduler::new();
        let mut tasks = TaskTable::new();
        let mut router = ipc::Router::new(0);
        let mut as_manager = AddressSpaceManager::new();
        let timer = MockTimer::default();

        // Slot 48 is empty (no capability).
        // The task has an address space but no MMIO capability.
        let kernel_as = as_manager.create().unwrap();
        as_manager.attach(kernel_as, task::Pid::KERNEL).unwrap();
        tasks.bootstrap_mut().address_space = Some(kernel_as);

        let mut ctx =
            Context::new(&mut scheduler, &mut tasks, &mut router, &mut as_manager, &timer);
        let mut table = SyscallTable::new();
        install_handlers(&mut table);

        // Attempt to map via empty slot 48.
        let args = Args::new([
            48,          // slot (empty)
            0x2000_0000, // va (page-aligned)
            0,           // offset
            0,
            0,
            0,
        ]);
        let err = table.dispatch(SYSCALL_MMIO_MAP, &mut ctx, &args).unwrap_err();
        assert_eq!(err, Error::Capability(CapError::InvalidSlot));
    }

    /// Test that mapping with the wrong capability kind (Endpoint instead of DeviceMmio) is rejected.
    /// Security invariant: Only DeviceMmio capabilities can be used for MMIO mapping.
    #[test]
    fn test_reject_mmio_wrong_cap_kind() {
        use super::SYSCALL_MMIO_MAP;

        let mut scheduler = Scheduler::new();
        let mut tasks = TaskTable::new();
        let mut router = ipc::Router::new(0);
        let mut as_manager = AddressSpaceManager::new();
        let timer = MockTimer::default();

        // Set up an Endpoint capability in slot 48 (wrong kind for MMIO).
        {
            let caps = tasks.bootstrap_mut().caps_mut();
            caps.set(
                48,
                Capability {
                    kind: CapabilityKind::Endpoint(0),
                    rights: Rights::MAP | Rights::SEND | Rights::RECV,
                },
            )
            .unwrap();
        }

        let kernel_as = as_manager.create().unwrap();
        as_manager.attach(kernel_as, task::Pid::KERNEL).unwrap();
        tasks.bootstrap_mut().address_space = Some(kernel_as);

        let mut ctx =
            Context::new(&mut scheduler, &mut tasks, &mut router, &mut as_manager, &timer);
        let mut table = SyscallTable::new();
        install_handlers(&mut table);

        // Attempt to map via slot 48 (Endpoint, not DeviceMmio).
        let args = Args::new([
            48,          // slot (has Endpoint, not DeviceMmio)
            0x2000_0000, // va (page-aligned)
            0,           // offset
            0,
            0,
            0,
        ]);
        let err = table.dispatch(SYSCALL_MMIO_MAP, &mut ctx, &args).unwrap_err();
        assert_eq!(err, Error::Capability(CapError::PermissionDenied));
    }

    /// Test that mapping beyond the device window bounds is rejected.
    /// Security invariant: MMIO mappings must be bounded to the device window.
    #[test]
    fn test_reject_mmio_outside_window() {
        use super::SYSCALL_MMIO_MAP;

        let mut scheduler = Scheduler::new();
        let mut tasks = TaskTable::new();
        let mut router = ipc::Router::new(0);
        let mut as_manager = AddressSpaceManager::new();
        let timer = MockTimer::default();

        // Set up a DeviceMmio capability with a small window (2 pages = 0x2000 bytes).
        const MMIO_BASE: usize = 0x1000_0000;
        const MMIO_LEN: usize = 0x2000; // 2 pages
        {
            let caps = tasks.bootstrap_mut().caps_mut();
            caps.set(
                48,
                Capability {
                    kind: CapabilityKind::DeviceMmio { base: MMIO_BASE, len: MMIO_LEN },
                    rights: Rights::MAP,
                },
            )
            .unwrap();
        }

        let kernel_as = as_manager.create().unwrap();
        as_manager.attach(kernel_as, task::Pid::KERNEL).unwrap();
        tasks.bootstrap_mut().address_space = Some(kernel_as);

        let mut ctx =
            Context::new(&mut scheduler, &mut tasks, &mut router, &mut as_manager, &timer);
        let mut table = SyscallTable::new();
        install_handlers(&mut table);

        // Attempt to map at offset 0x2000 (equals len, therefore out of bounds).
        let args = Args::new([
            48,          // slot (DeviceMmio)
            0x2000_0000, // va (page-aligned)
            MMIO_LEN,    // offset = len (out of bounds)
            0,
            0,
            0,
        ]);
        let err = table.dispatch(SYSCALL_MMIO_MAP, &mut ctx, &args).unwrap_err();
        assert_eq!(err, Error::Capability(CapError::PermissionDenied));

        // Also test offset way beyond the window.
        let args_far = Args::new([
            48,          // slot
            0x2000_0000, // va
            0x1_0000,    // offset = 64KiB (way beyond 8KiB window)
            0,
            0,
            0,
        ]);
        let err_far = table.dispatch(SYSCALL_MMIO_MAP, &mut ctx, &args_far).unwrap_err();
        assert_eq!(err_far, Error::Capability(CapError::PermissionDenied));
    }

    /// Test that mapping without MAP rights is rejected.
    /// Security invariant: MMIO mapping requires Rights::MAP.
    #[test]
    fn test_reject_mmio_insufficient_rights() {
        use super::SYSCALL_MMIO_MAP;

        let mut scheduler = Scheduler::new();
        let mut tasks = TaskTable::new();
        let mut router = ipc::Router::new(0);
        let mut as_manager = AddressSpaceManager::new();
        let timer = MockTimer::default();

        // Set up a DeviceMmio capability WITHOUT Rights::MAP (only SEND, which is meaningless).
        const MMIO_BASE: usize = 0x1000_0000;
        const MMIO_LEN: usize = 0x2000;
        {
            let caps = tasks.bootstrap_mut().caps_mut();
            caps.set(
                48,
                Capability {
                    kind: CapabilityKind::DeviceMmio { base: MMIO_BASE, len: MMIO_LEN },
                    rights: Rights::SEND, // Wrong rights for MMIO
                },
            )
            .unwrap();
        }

        let kernel_as = as_manager.create().unwrap();
        as_manager.attach(kernel_as, task::Pid::KERNEL).unwrap();
        tasks.bootstrap_mut().address_space = Some(kernel_as);

        let mut ctx =
            Context::new(&mut scheduler, &mut tasks, &mut router, &mut as_manager, &timer);
        let mut table = SyscallTable::new();
        install_handlers(&mut table);

        // Attempt to map with insufficient rights.
        let args = Args::new([
            48,          // slot
            0x2000_0000, // va
            0,           // offset (valid)
            0,
            0,
            0,
        ]);
        let err = table.dispatch(SYSCALL_MMIO_MAP, &mut ctx, &args).unwrap_err();
        assert_eq!(err, Error::Capability(CapError::PermissionDenied));
    }

    /// Test that executable MMIO mappings are rejected (no EXEC in leaf flags).
    /// Security invariant: device MMIO is USER|RW only (never executable).
    #[test]
    fn test_reject_mmio_exec() {
        use super::SYSCALL_MMIO_MAP;
        use crate::mm::page_table::PageFlags;

        let mut scheduler = Scheduler::new();
        let mut tasks = TaskTable::new();
        let mut router = ipc::Router::new(0);
        let mut as_manager = AddressSpaceManager::new();
        let timer = MockTimer::default();

        const MMIO_BASE: usize = 0x1000_0000;
        const MMIO_LEN: usize = 0x1000;
        const MMIO_VA: usize = 0x2000_0000;
        {
            let caps = tasks.bootstrap_mut().caps_mut();
            caps.set(
                48,
                Capability {
                    kind: CapabilityKind::DeviceMmio { base: MMIO_BASE, len: MMIO_LEN },
                    rights: Rights::MAP,
                },
            )
            .unwrap();
        }

        let kernel_as = as_manager.create().unwrap();
        as_manager.attach(kernel_as, task::Pid::KERNEL).unwrap();
        tasks.bootstrap_mut().address_space = Some(kernel_as);

        let mut ctx =
            Context::new(&mut scheduler, &mut tasks, &mut router, &mut as_manager, &timer);
        let mut table = SyscallTable::new();
        install_handlers(&mut table);

        let args = Args::new([48, MMIO_VA, 0, 0, 0, 0]);
        table.dispatch(SYSCALL_MMIO_MAP, &mut ctx, &args).unwrap();

        let handle = ctx.tasks.current_task().address_space().unwrap();
        let flags =
            ctx.address_spaces.get(handle).unwrap().page_table().leaf_flags(MMIO_VA).unwrap();
        assert!(flags.contains(PageFlags::USER));
        assert!(flags.contains(PageFlags::READ));
        assert!(flags.contains(PageFlags::WRITE));
        assert!(!flags.contains(PageFlags::EXECUTE));
    }

    /// Test that non-page-aligned virtual addresses are rejected.
    #[test]
    fn test_reject_mmio_unaligned_va() {
        use super::SYSCALL_MMIO_MAP;
        use crate::mm::address_space::AddressSpaceError;

        let mut scheduler = Scheduler::new();
        let mut tasks = TaskTable::new();
        let mut router = ipc::Router::new(0);
        let mut as_manager = AddressSpaceManager::new();
        let timer = MockTimer::default();

        const MMIO_BASE: usize = 0x1000_0000;
        const MMIO_LEN: usize = 0x1000;
        {
            let caps = tasks.bootstrap_mut().caps_mut();
            caps.set(
                48,
                Capability {
                    kind: CapabilityKind::DeviceMmio { base: MMIO_BASE, len: MMIO_LEN },
                    rights: Rights::MAP,
                },
            )
            .unwrap();
        }

        let kernel_as = as_manager.create().unwrap();
        as_manager.attach(kernel_as, task::Pid::KERNEL).unwrap();
        tasks.bootstrap_mut().address_space = Some(kernel_as);

        let mut ctx =
            Context::new(&mut scheduler, &mut tasks, &mut router, &mut as_manager, &timer);
        let mut table = SyscallTable::new();
        install_handlers(&mut table);

        let args = Args::new([48, 0x2000_0001, 0, 0, 0, 0]); // va not page-aligned
        let err = table.dispatch(SYSCALL_MMIO_MAP, &mut ctx, &args).unwrap_err();
        assert_eq!(err, Error::AddressSpace(AddressSpaceError::InvalidArgs));
    }

    /// Test that non-page-aligned offsets are rejected.
    #[test]
    fn test_reject_mmio_unaligned_offset() {
        use super::SYSCALL_MMIO_MAP;

        let mut scheduler = Scheduler::new();
        let mut tasks = TaskTable::new();
        let mut router = ipc::Router::new(0);
        let mut as_manager = AddressSpaceManager::new();
        let timer = MockTimer::default();

        const MMIO_BASE: usize = 0x1000_0000;
        const MMIO_LEN: usize = 0x2000;
        {
            let caps = tasks.bootstrap_mut().caps_mut();
            caps.set(
                48,
                Capability {
                    kind: CapabilityKind::DeviceMmio { base: MMIO_BASE, len: MMIO_LEN },
                    rights: Rights::MAP,
                },
            )
            .unwrap();
        }

        let kernel_as = as_manager.create().unwrap();
        as_manager.attach(kernel_as, task::Pid::KERNEL).unwrap();
        tasks.bootstrap_mut().address_space = Some(kernel_as);

        let mut ctx =
            Context::new(&mut scheduler, &mut tasks, &mut router, &mut as_manager, &timer);
        let mut table = SyscallTable::new();
        install_handlers(&mut table);

        let args = Args::new([48, 0x2000_0000, 1, 0, 0, 0]); // offset not page-aligned
        let err = table.dispatch(SYSCALL_MMIO_MAP, &mut ctx, &args).unwrap_err();
        assert_eq!(err, Error::Capability(CapError::PermissionDenied));
    }

    /// Test that remapping the same VA deterministically fails (no silent overwrite).
    #[test]
    fn test_reject_mmio_overlap_same_va() {
        use super::SYSCALL_MMIO_MAP;
        use crate::mm::{address_space::AddressSpaceError, page_table::MapError};

        let mut scheduler = Scheduler::new();
        let mut tasks = TaskTable::new();
        let mut router = ipc::Router::new(0);
        let mut as_manager = AddressSpaceManager::new();
        let timer = MockTimer::default();

        const MMIO_BASE: usize = 0x1000_0000;
        const MMIO_LEN: usize = 0x2000;
        const MMIO_VA: usize = 0x2000_0000;
        {
            let caps = tasks.bootstrap_mut().caps_mut();
            caps.set(
                48,
                Capability {
                    kind: CapabilityKind::DeviceMmio { base: MMIO_BASE, len: MMIO_LEN },
                    rights: Rights::MAP,
                },
            )
            .unwrap();
        }

        let kernel_as = as_manager.create().unwrap();
        as_manager.attach(kernel_as, task::Pid::KERNEL).unwrap();
        tasks.bootstrap_mut().address_space = Some(kernel_as);

        let mut ctx =
            Context::new(&mut scheduler, &mut tasks, &mut router, &mut as_manager, &timer);
        let mut table = SyscallTable::new();
        install_handlers(&mut table);

        let args = Args::new([48, MMIO_VA, 0, 0, 0, 0]);
        table.dispatch(SYSCALL_MMIO_MAP, &mut ctx, &args).unwrap();

        // Second map to the same VA must fail (overlap).
        let err = table.dispatch(SYSCALL_MMIO_MAP, &mut ctx, &args).unwrap_err();
        assert_eq!(err, Error::AddressSpace(AddressSpaceError::Mapping(MapError::Overlap)));
    }
}
