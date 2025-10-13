// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Task table and lifecycle helpers for the NEURON kernel.

extern crate alloc;

use alloc::vec;
use alloc::vec::Vec;

use crate::{
    bootstrap::BootstrapMsg,
    cap::{CapError, CapTable, CapabilityKind, Rights},
    ipc::{self, header::MessageHeader, Message, Router},
    mm::{AddressSpaceError, AddressSpaceManager, AsHandle, PageFlags, PAGE_SIZE},
    sched::{QosClass, Scheduler},
    trap::TrapFrame,
};
use spin::Mutex;

/// Process identifier.
pub type Pid = u32;

const USER_STACK_TOP: usize = 0x4000_0000;
const STACK_PAGES: usize = 4;
const STACK_POOL_BASE: usize = 0x8000_0000 + 0x10_0000;
const STACK_POOL_LIMIT: usize = 0x8000_0000 + 0x20_0000;

struct StackPool {
    cursor: usize,
}

impl StackPool {
    const fn new() -> Self {
        Self { cursor: STACK_POOL_LIMIT }
    }

    fn alloc(&mut self, pages: usize) -> Option<usize> {
        let bytes = pages.checked_mul(PAGE_SIZE)?;
        let next = self.cursor.checked_sub(bytes)?;
        if next < STACK_POOL_BASE {
            None
        } else {
            self.cursor = next;
            Some(next)
        }
    }
}

static STACK_ALLOCATOR: Mutex<StackPool> = Mutex::new(StackPool::new());

fn allocate_guarded_stack(
    address_spaces: &mut AddressSpaceManager,
    handle: AsHandle,
) -> Result<usize, SpawnError> {
    let phys_base = {
        let mut pool = STACK_ALLOCATOR.lock();
        pool.alloc(STACK_PAGES).ok_or(SpawnError::StackExhausted)?
    };
    let flags = PageFlags::VALID | PageFlags::READ | PageFlags::WRITE | PageFlags::USER;
    let guard_bottom = USER_STACK_TOP - (STACK_PAGES + 1) * PAGE_SIZE;
    for page in 0..STACK_PAGES {
        let page_va = guard_bottom + PAGE_SIZE + page * PAGE_SIZE;
        let page_pa = phys_base + page * PAGE_SIZE;
        address_spaces.map_page(handle, page_va, page_pa, flags)?;
    }
    Ok(USER_STACK_TOP)
}

/// Error returned when spawning a new task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpawnError {
    /// Parent PID does not exist.
    InvalidParent,
    /// Entry point does not fit in the machine address space.
    InvalidEntryPoint,
    /// Stack pointer does not fit in the machine address space.
    InvalidStackPointer,
    /// Bootstrap capability was not an endpoint.
    BootstrapNotEndpoint,
    /// Capability lookup failed.
    Capability(CapError),
    /// IPC enqueue failed while delivering the bootstrap message.
    Ipc(ipc::IpcError),
    /// Address-space operation failed.
    AddressSpace(AddressSpaceError),
    /// Stack allocator ran out of physical pages.
    StackExhausted,
}

impl From<CapError> for SpawnError {
    fn from(value: CapError) -> Self {
        Self::Capability(value)
    }
}

impl From<ipc::IpcError> for SpawnError {
    fn from(value: ipc::IpcError) -> Self {
        Self::Ipc(value)
    }
}

impl From<AddressSpaceError> for SpawnError {
    fn from(value: AddressSpaceError) -> Self {
        Self::AddressSpace(value)
    }
}

/// Error returned when transferring capabilities between tasks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferError {
    /// Parent PID does not exist.
    InvalidParent,
    /// Child PID does not exist.
    InvalidChild,
    /// Capability operation failed.
    Capability(CapError),
}

impl From<CapError> for TransferError {
    fn from(value: CapError) -> Self {
        Self::Capability(value)
    }
}

/// Minimal task control block.
#[derive(Clone)]
pub struct Task {
    #[allow(dead_code)]
    pid: Pid,
    parent: Option<Pid>,
    frame: TrapFrame,
    caps: CapTable,
    /// Handle referencing the address space bound to this task.
    pub address_space: Option<AsHandle>,
    bootstrap_slot: Option<usize>,
}

impl Task {
    fn bootstrap() -> Self {
        Self {
            pid: 0,
            parent: None,
            frame: TrapFrame::default(),
            caps: CapTable::new(),
            address_space: None,
            bootstrap_slot: None,
        }
    }

    /// Returns the saved trap frame.
    pub fn frame(&self) -> &TrapFrame {
        &self.frame
    }

    /// Returns a mutable view of the trap frame.
    pub fn frame_mut(&mut self) -> &mut TrapFrame {
        &mut self.frame
    }

    /// Returns a mutable reference to the capability table.
    pub fn caps_mut(&mut self) -> &mut CapTable {
        &mut self.caps
    }

    /// Returns the parent PID, if any.
    pub fn parent(&self) -> Option<Pid> {
        self.parent
    }

    /// Returns the address-space handle bound to this task, if any.
    pub fn address_space(&self) -> Option<AsHandle> {
        self.address_space
    }

    /// Returns the slot that seeded the bootstrap endpoint, if any.
    pub fn bootstrap_slot(&self) -> Option<usize> {
        self.bootstrap_slot
    }
}

/// Kernel task table managing task control blocks.
pub struct TaskTable {
    tasks: Vec<Task>,
    current: Pid,
}

impl TaskTable {
    /// Creates a new table seeded with the bootstrap task (PID 0).
    pub fn new() -> Self {
        crate::uart::write_line("TT: new enter");
        let table = Self { tasks: vec![Task::bootstrap()], current: 0 };
        crate::uart::write_line("TT: new exit");
        table
    }

    /// Returns the PID of the currently running task.
    pub fn current_pid(&self) -> Pid {
        self.current
    }

    /// Changes the currently running task.
    pub fn set_current(&mut self, pid: Pid) {
        self.current = pid;
    }

    /// Returns a mutable reference to the bootstrap task (PID 0).
    pub fn bootstrap_mut(&mut self) -> &mut Task {
        &mut self.tasks[0]
    }

    /// Returns a shared reference to the current task.
    pub fn current_task(&self) -> &Task {
        &self.tasks[self.current as usize]
    }

    /// Returns a mutable reference to the current task.
    pub fn current_task_mut(&mut self) -> &mut Task {
        &mut self.tasks[self.current as usize]
    }

    /// Returns a shared reference to a task by PID.
    pub fn task(&self, pid: Pid) -> Option<&Task> {
        self.tasks.get(pid as usize)
    }

    /// Returns the capability table of the current task.
    pub fn current_caps_mut(&mut self) -> &mut CapTable {
        self.current_task_mut().caps_mut()
    }

    /// Returns a shared reference to the capability table of `pid`.
    pub fn caps_of(&self, pid: Pid) -> Option<&CapTable> {
        self.tasks.get(pid as usize).map(|task| &task.caps)
    }

    /// Returns a mutable reference to the capability table of `pid`.
    pub fn caps_of_mut(&mut self, pid: Pid) -> Option<&mut CapTable> {
        self.tasks.get_mut(pid as usize).map(|task| task.caps_mut())
    }

    /// Returns a mutable reference to a task by PID.
    pub fn task_mut(&mut self, pid: Pid) -> Option<&mut Task> {
        self.tasks.get_mut(pid as usize)
    }

    /// Spawns a child task that temporarily shares its parent's address space.
    #[inline(always)]
    pub fn spawn(
        &mut self,
        parent: Pid,
        entry_pc: u64,
        stack_sp: u64,
        address_space: Option<AsHandle>,
        bootstrap_slot: u32,
        scheduler: &mut Scheduler,
        router: &mut Router,
        address_spaces: &mut AddressSpaceManager,
    ) -> Result<Pid, SpawnError> {
        // Keep the wrapper minimal to reduce prologue pressure; delegate to the helper.
        self.spawn_inner(
            parent,
            entry_pc,
            stack_sp,
            address_space,
            bootstrap_slot,
            scheduler,
            router,
            address_spaces,
        )
    }

    /// Helper containing the actual spawn logic. Kept separate to allow a minimal wrapper.
    #[inline(never)]
    fn spawn_inner(
        &mut self,
        parent: Pid,
        entry_pc: u64,
        stack_sp: u64,
        address_space: Option<AsHandle>,
        bootstrap_slot: u32,
        scheduler: &mut Scheduler,
        router: &mut Router,
        address_spaces: &mut AddressSpaceManager,
    ) -> Result<Pid, SpawnError> {
        {
            use core::fmt::Write as _;
            let mut w = crate::uart::raw_writer();
            let _ = write!(
                w,
                "SPAWN-I: start parent={} entry=0x{:x} sp=0x{:x}\n",
                parent, entry_pc, stack_sp
            );
        }
        let parent_index = parent as usize;
        let parent_task = self.tasks.get(parent_index).ok_or(SpawnError::InvalidParent)?;

        if entry_pc > usize::MAX as u64 {
            return Err(SpawnError::InvalidEntryPoint);
        }
        if stack_sp > usize::MAX as u64 {
            return Err(SpawnError::InvalidStackPointer);
        }

        // Validate entry point lies within kernel text (RX) for OS selftest stage
        #[cfg(all(target_arch = "riscv64", target_os = "none"))]
        unsafe {
            extern "C" {
                static __text_start: u8;
                static __text_end: u8;
            }
            let start = &__text_start as *const u8 as usize;
            let end = &__text_end as *const u8 as usize;
            let pc = entry_pc as usize;
            // Only enforce range if linker provided sane bounds
            if end > start {
                if pc < start || pc >= end || (pc & 0x1) != 0 {
                    use core::fmt::Write as _;
                    let mut w = crate::uart::raw_writer();
                    let _ =
                        write!(w, "SPAWN-E: pc=0x{:x} start=0x{:x} end=0x{:x}\n", pc, start, end);
                    return Err(SpawnError::InvalidEntryPoint);
                }
            }
        }
        // Warn if stack is zero during bring-up (we allow 0 for MVP shared-AS test)
        if stack_sp == 0 {
            use core::fmt::Write as _;
            let mut w = crate::uart::raw_writer();
            let _ = write!(w, "SPAWN-W: stack sp=0 (MVP shared-AS)\n");
        }

        let slot = bootstrap_slot as usize;
        let bootstrap_cap = parent_task.caps.get(slot)?;
        let endpoint = match bootstrap_cap.kind {
            CapabilityKind::Endpoint(id) => id,
            _ => return Err(SpawnError::BootstrapNotEndpoint),
        };
        {
            use core::fmt::Write as _;
            let mut w = crate::uart::raw_writer();
            let _ = write!(w, "SPAWN-I: bootstrap cap ok ep={} slot={}\n", endpoint, slot);
        }

        let mut child_caps = CapTable::new();
        child_caps.set(slot, bootstrap_cap)?;

        let mut frame = TrapFrame::default();
        frame.sepc = entry_pc as usize;
        let (child_as, sp_value) = match address_space {
            Some(handle) => {
                let sp = if stack_sp == 0 { USER_STACK_TOP as u64 } else { stack_sp };
                (handle, sp as usize)
            }
            None => {
                let handle = address_spaces.create()?;
                let top = allocate_guarded_stack(address_spaces, handle)?;
                (handle, top)
            }
        };
        frame.x[2] = sp_value;
        const SSTATUS_SPIE: usize = 1 << 5;
        const SSTATUS_SPP: usize = 1 << 8;
        frame.sstatus &= !SSTATUS_SPP;
        frame.sstatus |= SSTATUS_SPIE;
        {
            use core::fmt::Write as _;
            let mut w = crate::uart::raw_writer();
            let _ = write!(w, "SPAWN-I: frame set sepc=0x{:x} sp=0x{:x}\n", frame.sepc, frame.x[2]);
        }

        let pid = self.tasks.len() as Pid;
        let task = Task {
            pid,
            parent: Some(parent),
            frame,
            caps: child_caps,
            address_space: Some(child_as),
            bootstrap_slot: Some(slot),
        };
        self.tasks.push(task);
        if let Err(err) = address_spaces.attach(child_as, pid) {
            self.tasks.pop();
            return Err(err.into());
        }
        {
            use core::fmt::Write as _;
            let mut w = crate::uart::raw_writer();
            let _ = write!(w, "SPAWN-I: task created pid={}\n", pid);
        }

        scheduler.enqueue(pid, QosClass::Normal);
        {
            use core::fmt::Write as _;
            let mut w = crate::uart::raw_writer();
            let _ = write!(w, "SPAWN-I: enqueued pid={} qos=normal\n", pid);
        }

        let bootstrap = BootstrapMsg::default();
        let payload = bootstrap.to_le_bytes().to_vec();
        let len = payload.len() as u32;
        let header = MessageHeader::new(parent, endpoint, 0, 0, len);
        {
            use core::fmt::Write as _;
            let mut w = crate::uart::raw_writer();
            let _ = write!(w, "SPAWN: before send ep={} len={}\n", endpoint, len);
        }
        router.send(endpoint, Message::new(header, payload))?;

        Ok(pid)
    }

    /// Duplicates a capability from `parent` into `child` with a rights subset.
    pub fn transfer_cap(
        &mut self,
        parent: Pid,
        child: Pid,
        parent_slot: usize,
        rights: Rights,
    ) -> Result<usize, TransferError> {
        let parent_task = self.tasks.get(parent as usize).ok_or(TransferError::InvalidParent)?;
        let derived = parent_task.caps.derive(parent_slot, rights)?;
        let child_task = self.tasks.get_mut(child as usize).ok_or(TransferError::InvalidChild)?;
        child_task.caps_mut().allocate(derived).map_err(TransferError::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cap::{CapError, Capability, CapabilityKind, Rights};
    use crate::ipc::Router;
    use crate::mm::AddressSpaceManager;
    use crate::sched::Scheduler;

    #[test]
    fn bootstrap_task_present() {
        let table = TaskTable::new();
        assert_eq!(table.current_pid(), 0);
        assert_eq!(table.current_task().pid, 0);
    }

    #[test]
    fn transfer_capability_respects_rights() {
        let mut table = TaskTable::new();
        {
            let caps = table.bootstrap_mut().caps_mut();
            caps.set(
                0,
                Capability {
                    kind: CapabilityKind::Endpoint(0),
                    rights: Rights::SEND | Rights::RECV,
                },
            )
            .unwrap();
        }
        let mut scheduler = Scheduler::new();
        let mut router = Router::new(1);
        let mut spaces = AddressSpaceManager::new();
        let bootstrap_as = spaces.create().unwrap();
        spaces.attach(bootstrap_as, 0).unwrap();
        table.bootstrap_mut().address_space = Some(bootstrap_as);
        let child = table
            .spawn(0, 0, 0, None, 0, &mut scheduler, &mut router, &mut spaces)
            .unwrap();

        let slot = table.transfer_cap(0, child, 0, Rights::RECV).unwrap();
        assert_ne!(slot, 0);
        let cap = table.caps_of(child).unwrap().get(slot).unwrap();
        assert_eq!(cap.kind, CapabilityKind::Endpoint(0));
        assert_eq!(cap.rights, Rights::RECV);

        let err = table.transfer_cap(0, child, 0, Rights::MAP).unwrap_err();
        assert_eq!(err, TransferError::Capability(CapError::PermissionDenied));
    }
}
