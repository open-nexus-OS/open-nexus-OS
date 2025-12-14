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
        Ok(Self {
            elf_ptr,
            elf_len,
            stack_pages,
            global_pointer,
        })
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
        Ok(Self {
            entry_pc,
            stack_sp,
            as_handle,
            bootstrap_slot,
            global_pointer,
        })
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
        Ok(Self {
            handle,
            vmo_slot,
            va,
            len,
            prot,
            flags,
        })
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
        Ok(Self {
            slot: SlotIndex::decode(args.get(0)),
        })
    }
    #[inline]
    fn check(&self) -> Result<(), Error> {
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
struct VmoCreateArgsTyped {
    slot_raw: usize,
    len: usize,
}

impl VmoCreateArgsTyped {
    #[inline]
    fn decode(args: &Args) -> Result<Self, Error> {
        Ok(Self {
            slot_raw: args.get(0),
            len: args.get(1),
        })
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
    SYSCALL_DEBUG_PUTC, SYSCALL_EXIT, SYSCALL_EXEC, SYSCALL_MAP, SYSCALL_NSEC, SYSCALL_RECV,
    SYSCALL_SEND, SYSCALL_SPAWN, SYSCALL_VMO_CREATE, SYSCALL_VMO_WRITE, SYSCALL_WAIT,
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
        Self {
            scheduler,
            tasks,
            router,
            address_spaces,
            timer,
            last_message: None,
        }
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
    table.register(SYSCALL_EXIT, sys_exit);
    table.register(SYSCALL_WAIT, sys_wait);
    table.register(SYSCALL_EXEC, sys_exec);
    table.register(SYSCALL_DEBUG_PUTC, sys_debug_putc);
    {
        use core::fmt::Write as _;
        let mut u = crate::uart::raw_writer();
        let _ = u.write_str("SYSCALL install debug_putc=0x");
        crate::trap::uart_write_hex(&mut u, sys_debug_putc as usize);
        let _ = u.write_str("\n");
    }
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
                let _ = write!(
                    w,
                    "YIELD-I: next pid={} sepc=0x{:x}\n",
                    next,
                    task.frame().sepc
                );
            }
            #[cfg(not(feature = "debug_uart"))]
            let _ = task; // silence unused when debug UART is disabled
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
    let typed = SendArgsTyped::decode(args)?;
    typed.check()?;
    let cap = ctx
        .tasks
        .current_caps_mut()
        .derive(typed.slot.0, Rights::SEND)?;
    let endpoint = match cap.kind {
        CapabilityKind::Endpoint(id) => id,
        _ => return Err(Error::Capability(CapError::PermissionDenied)),
    };
    let header = MessageHeader::new(
        typed.slot.0 as u32,
        endpoint,
        typed.ty,
        typed.flags,
        typed.len,
    );
    let payload = Vec::new();
    ctx.router
        .send(endpoint, ipc::Message::new(header, payload))?;
    Ok(typed.len as usize)
}

fn sys_recv(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let typed = RecvArgsTyped::decode(args)?;
    typed.check()?;
    let cap = ctx
        .tasks
        .current_caps_mut()
        .derive(typed.slot.0, Rights::RECV)?;
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
    let typed = MapArgsTyped::decode(args)?;
    typed.check()?;
    let cap = ctx
        .tasks
        .current_caps_mut()
        .derive(typed.slot.0, Rights::MAP)?;
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
            ctx.address_spaces
                .map_page(handle, va.raw(), pa, typed.flags)?;
            Ok(0)
        }
        _ => Err(Error::Capability(CapError::PermissionDenied)),
    }
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
    let cap = Capability {
        kind: CapabilityKind::Vmo {
            base,
            len: aligned_len,
        },
        rights: Rights::MAP,
    };
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
    let cap = ctx
        .tasks
        .current_caps_mut()
        .derive(typed.slot.0, Rights::MAP)?;
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
    let span_end = typed
        .offset
        .checked_add(typed.len)
        .ok_or(Error::Capability(CapError::PermissionDenied))?;
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
        let _ = write!(
            u,
            "VMO-WRITE DATA slot=0x{:x} off=0x{:x} head=0x",
            typed.slot.0, typed.offset
        );
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
    ctx.tasks.exit_current(status);
    ctx.scheduler.finish_current();
    if let Some(next) = ctx.scheduler.schedule_next() {
        ctx.tasks.set_current(next as task::Pid);
        if let Some(task) = ctx.tasks.task(next as task::Pid) {
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
        ctx.tasks.set_current(0);
    }
    Err(Error::TaskExit)
}

fn sys_wait(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let raw_pid = args.get(0) as i32;
    let target = if raw_pid <= 0 {
        None
    } else {
        Some(raw_pid as task::Pid)
    };
    loop {
        match ctx.tasks.reap_child(target, ctx.address_spaces) {
            Ok((pid, status)) => {
                if let Some(task) = ctx.tasks.task_mut(ctx.tasks.current_pid()) {
                    task.frame_mut().x[11] = status as usize;
                }
                return Ok(pid as usize);
            }
            Err(task::WaitError::WouldBlock) => {
                let zero_args = Args::new([0; 6]);
                let _ = sys_yield(ctx, &zero_args)?;
            }
            Err(err) => return Err(Error::from(err)),
        }
    }
}

fn read_u16_le(bytes: &[u8], off: usize) -> Result<u16, Error> {
    let end = off
        .checked_add(2)
        .ok_or(AddressSpaceError::InvalidArgs)?;
    let slice = bytes.get(off..end).ok_or(AddressSpaceError::InvalidArgs)?;
    Ok(u16::from_le_bytes([slice[0], slice[1]]))
}

fn read_u32_le(bytes: &[u8], off: usize) -> Result<u32, Error> {
    let end = off
        .checked_add(4)
        .ok_or(AddressSpaceError::InvalidArgs)?;
    let slice = bytes.get(off..end).ok_or(AddressSpaceError::InvalidArgs)?;
    Ok(u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

fn read_u64_le(bytes: &[u8], off: usize) -> Result<u64, Error> {
    let end = off
        .checked_add(8)
        .ok_or(AddressSpaceError::InvalidArgs)?;
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

    const PT_LOAD: u32 = 1;
    const PF_R: u32 = 4;
    const PF_W: u32 = 2;
    const PF_X: u32 = 1;

    let as_handle = ctx.address_spaces.create()?;

    // Map PT_LOAD segments
    for i in 0..e_phnum {
        let off = e_phoff
            .checked_add(i * e_phentsize)
            .ok_or(AddressSpaceError::InvalidArgs)?;
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
        let end = p_offset
            .checked_add(p_filesz)
            .ok_or(AddressSpaceError::InvalidArgs)?;
        if end > elf.len() {
            return Err(AddressSpaceError::InvalidArgs.into());
        }
        if (p_flags & PF_W != 0) && (p_flags & PF_X != 0) {
            return Err(AddressSpaceError::from(MapError::PermissionDenied).into());
        }

        let page_off = p_vaddr & (PAGE_SIZE - 1);
        let aligned_vaddr = p_vaddr - page_off;
        let alloc_len = align_len(p_memsz.checked_add(page_off).ok_or(AddressSpaceError::InvalidArgs)?)
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
        // Zero BSS tail
        if p_memsz > p_filesz {
            let bss_start = base
                .checked_add(page_off)
                .and_then(|v| v.checked_add(p_filesz))
                .ok_or(AddressSpaceError::InvalidArgs)?;
            let bss_len = p_memsz
                .checked_sub(p_filesz)
                .ok_or(AddressSpaceError::InvalidArgs)?;
            unsafe {
                ptr::write_bytes(bss_start as *mut u8, 0, bss_len);
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

        // Map pages
        let pages = alloc_len / PAGE_SIZE;
        for page in 0..pages {
            let va = aligned_vaddr
                .checked_add(page * PAGE_SIZE)
                .ok_or(AddressSpaceError::InvalidArgs)?;
            let pa = base
                .checked_add(page * PAGE_SIZE)
                .ok_or(AddressSpaceError::InvalidArgs)?;
            ctx.address_spaces.map_page(as_handle, va, pa, flags)?;
        }
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
    let stack_bytes = total_pages
        .checked_mul(PAGE_SIZE)
        .ok_or(AddressSpaceError::InvalidArgs)?;
    let (stack_base, stack_len) = VMO_POOL.lock().allocate(stack_bytes)?;
    // Clear the freshly allocated stack to avoid stale data influencing user
    // register setup/prologue logic.
    unsafe {
        ptr::write_bytes(stack_base as *mut u8, 0, stack_len);
    }
    let user_stack_top: usize = 0x2000_0000;
    // Map through the former faulting address (boundary) and leave a guard above.
    let mapped_top = user_stack_top
        .checked_add(10 * PAGE_SIZE)
        .ok_or(AddressSpaceError::InvalidArgs)?; // boundary page mapped; guard sits above
    let stack_bottom = mapped_top
        .checked_sub(stack_len)
        .ok_or(AddressSpaceError::InvalidArgs)?;

    let stack_flags = PageFlags::VALID | PageFlags::USER | PageFlags::READ | PageFlags::WRITE;
    for page in 0..total_pages {
        let va = stack_bottom
            .checked_add(page * PAGE_SIZE)
            .ok_or(AddressSpaceError::InvalidArgs)?;
        let pa = stack_base
            .checked_add(page * PAGE_SIZE)
            .ok_or(AddressSpaceError::InvalidArgs)?;
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

    let sp_probe = mapped_top
        .checked_sub(2 * PAGE_SIZE)
        .ok_or(AddressSpaceError::InvalidArgs)?;
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
        typed.global_pointer,
        bootstrap_slot,
        ctx.scheduler,
        ctx.router,
        ctx.address_spaces,
    )?;

    Ok(pid as usize)
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
    let pid = ctx.tasks.spawn(
        parent,
        typed.entry_pc,
        typed.stack_sp,
        typed.as_handle,
        typed.global_pointer,
        typed.bootstrap_slot,
        ctx.scheduler,
        ctx.router,
        ctx.address_spaces,
    )?;

    Ok(pid as usize)
}

fn sys_cap_transfer(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let typed = CapTransferArgsTyped::decode(args)?;
    let rights = typed.check()?;
    let parent = ctx.tasks.current_pid();
    let slot = ctx
        .tasks
        .transfer_cap(parent, typed.child, typed.parent_slot.0, rights)?;
    Ok(slot)
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
        Self {
            base: 0,
            next: 0,
            limit: 0,
        }
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
        let next = self
            .next
            .checked_add(aligned)
            .ok_or(Error::Capability(CapError::PermissionDenied))?;
        if next > self.limit {
            return Err(Error::Capability(CapError::PermissionDenied));
        }
        let base = self.next;
        self.next = next;
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
        len.checked_add(PAGE_SIZE - 1)
            .map(|value| value & !(PAGE_SIZE - 1))
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
    if ptr >= USER_VADDR_LIMIT {
        return Err(AddressSpaceError::InvalidArgs.into());
    }
    let last = ptr
        .checked_add(len - 1)
        .ok_or(AddressSpaceError::InvalidArgs)?;
    if last >= USER_VADDR_LIMIT {
        return Err(AddressSpaceError::InvalidArgs.into());
    }
    Ok(())
}

fn sys_as_create(ctx: &mut Context<'_>, _args: &Args) -> SysResult<usize> {
    let handle = ctx.address_spaces.create()?;
    Ok(handle.to_raw() as usize)
}

// TODO: Enforce W^X policy consistently for user mappings when flags/prot are extended.
fn sys_as_map(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let typed = AsMapArgsTyped::decode(args)?;
    typed.check()?; // Check phase

    let cap = ctx
        .tasks
        .current_caps_mut()
        .derive(typed.vmo_slot.0, Rights::MAP)?;
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
    typed
        .va
        .checked_add(span_bytes)
        .ok_or(AddressSpaceError::InvalidArgs)?;

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
        let page_va = typed
            .va
            .raw()
            .checked_add(page * PAGE_SIZE)
            .ok_or(AddressSpaceError::InvalidArgs)?;
        let page_pa = base
            .checked_add(page * PAGE_SIZE)
            .ok_or(AddressSpaceError::InvalidArgs)?;
        ctx.address_spaces
            .map_page(typed.handle, page_va, page_pa, flags)?;
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

        table
            .dispatch(SYSCALL_SEND, &mut ctx, &Args::new([0, 1, 0, 0, 0, 0]))
            .unwrap();
        let len = table
            .dispatch(SYSCALL_RECV, &mut ctx, &Args::new([0, 0, 0, 0, 0, 0]))
            .unwrap();
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
