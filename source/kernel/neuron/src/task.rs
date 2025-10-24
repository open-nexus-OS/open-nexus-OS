// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Task table and lifecycle helpers for the NEURON kernel
//! OWNERS: @kernel-sched-team
//! PUBLIC API: TaskTable (spawn/exit/wait), Pid, TaskState
//! DEPENDS_ON: ipc::Router, mm::AddressSpaceManager, sched::Scheduler, types::{SlotIndex,VirtAddr}
//! INVARIANTS: Kernel text entry validation; guard-paged user stacks; capability rights respected
//! ADR: docs/adr/0001-runtime-roles-and-boundaries.md

extern crate alloc;

use alloc::vec::Vec;

use crate::{
    cap::{CapError, CapTable, CapabilityKind, Rights},
    ipc::{self, Router},
    mm::{AddressSpaceError, AddressSpaceManager, AsHandle, PageFlags, PAGE_SIZE},
    sched::{QosClass, Scheduler},
    trap::TrapFrame,
    types::{SlotIndex, VirtAddr},
};
use spin::Mutex;

/// Process identifier.
pub type Pid = u32;

/// Lifecycle state of a task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskState {
    Running,
    Zombie,
}

/// Errors returned when waiting for child processes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitError {
    /// Current task has no children to reap.
    NoChildren,
    /// Requested PID is not a child of the current task.
    NoSuchPid,
    /// Wait argument is not valid (for example, waiting on self).
    InvalidTarget,
    /// Target child exists but has not exited yet.
    WouldBlock,
}

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
) -> Result<VirtAddr, SpawnError> {
    let phys_base = {
        let mut pool = STACK_ALLOCATOR.lock();
        pool.alloc(STACK_PAGES).ok_or(SpawnError::StackExhausted)?
    };
    let flags = PageFlags::VALID | PageFlags::READ | PageFlags::WRITE | PageFlags::USER;
    let guard_bottom = USER_STACK_TOP - (STACK_PAGES + 1) * PAGE_SIZE;
    #[cfg(feature = "debug_uart")]
    {
        use core::fmt::Write as _;
        let mut u = crate::uart::raw_writer();
        let _ = write!(
            u,
            "STACK: base=0x{:x} guard_bottom=0x{:x} pages={}\n",
            phys_base, guard_bottom, STACK_PAGES
        );
    }
    for page in 0..STACK_PAGES {
        let page_va = guard_bottom + PAGE_SIZE + page * PAGE_SIZE;
        let page_pa = phys_base + page * PAGE_SIZE;
        address_spaces.map_page(handle, page_va, page_pa, flags)?;
        #[cfg(feature = "debug_uart")]
        {
            use core::fmt::Write as _;
            let mut u = crate::uart::raw_writer();
            let _ = write!(u, "STACK: map idx={} va=0x{:x} pa=0x{:x}\n", page, page_va, page_pa);
        }
    }
    #[cfg(feature = "debug_uart")]
    {
        use core::fmt::Write as _;
        let mut u = crate::uart::raw_writer();
        let _ = write!(u, "STACK: top=0x{:x}\n", USER_STACK_TOP);
    }
    VirtAddr::page_aligned(USER_STACK_TOP).ok_or(SpawnError::InvalidStackPointer)
}

fn ensure_entry_in_kernel_text(entry: VirtAddr) -> Result<(), SpawnError> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        extern "C" {
            static __text_start: u8;
            static __text_end: u8;
        }
        let start = unsafe { &__text_start as *const u8 as usize };
        let end = unsafe { &__text_end as *const u8 as usize };
        let pc = entry.raw();
        if end <= start || pc < start || pc >= end {
            return Err(SpawnError::InvalidEntryPoint);
        }
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    let _ = entry;
    Ok(())
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
    #[cfg_attr(not(test), allow(dead_code))]
    pid: Pid,
    parent: Option<Pid>,
    state: TaskState,
    exit_code: Option<i32>,
    frame: TrapFrame,
    caps: CapTable,
    /// Handle referencing the address space bound to this task.
    pub address_space: Option<AsHandle>,
    bootstrap_slot: Option<usize>,
    children: Vec<Pid>,
}

impl Task {
    fn bootstrap() -> Self {
        let caps = CapTable::new();
        // Avoid Default impl to minimize any unexpected code paths during bring-up.
        let zero_frame = TrapFrame { x: [0; 32], sepc: 0, sstatus: 0, scause: 0, stval: 0 };
        let t = Self {
            pid: 0,
            parent: None,
            state: TaskState::Running,
            exit_code: None,
            frame: zero_frame,
            caps,
            address_space: None,
            bootstrap_slot: None,
            children: Vec::new(),
        };
        t
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

    /// Returns the lifecycle state of the task.
    pub fn state(&self) -> TaskState {
        self.state
    }

    /// Returns the stored exit code, if any.
    pub fn exit_code(&self) -> Option<i32> {
        self.exit_code
    }

    /// Returns the child list.
    pub fn children(&self) -> &[Pid] {
        &self.children
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
        let mut tasks_vec: Vec<Task> = Vec::new();
        tasks_vec.push(Task::bootstrap());
        Self { tasks: tasks_vec, current: 0 }
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
        entry_pc: VirtAddr,
        stack_sp: Option<VirtAddr>,
        address_space: Option<AsHandle>,
        bootstrap_slot: SlotIndex,
        scheduler: &mut Scheduler,
        _router: &mut Router,
        address_spaces: &mut AddressSpaceManager,
    ) -> Result<Pid, SpawnError> {
        self.spawn_inner(
            parent,
            entry_pc,
            stack_sp,
            address_space,
            bootstrap_slot,
            scheduler,
            _router,
            address_spaces,
        )
    }

    /// Helper containing the actual spawn logic. Kept separate to allow a minimal wrapper.
    #[inline(never)]
    fn spawn_inner(
        &mut self,
        parent: Pid,
        entry_pc: VirtAddr,
        stack_sp: Option<VirtAddr>,
        address_space: Option<AsHandle>,
        bootstrap_slot: SlotIndex,
        scheduler: &mut Scheduler,
        _router: &mut Router,
        address_spaces: &mut AddressSpaceManager,
    ) -> Result<Pid, SpawnError> {
        let parent_index = parent as usize;
        let parent_task = self.tasks.get(parent_index).ok_or(SpawnError::InvalidParent)?;

        ensure_entry_in_kernel_text(entry_pc)?;

        let slot = bootstrap_slot.0;
        let bootstrap_cap = parent_task.caps.get(slot)?;
        match bootstrap_cap.kind {
            CapabilityKind::Endpoint(_) => {}
            _ => return Err(SpawnError::BootstrapNotEndpoint),
        }

        let mut child_caps = CapTable::new();
        child_caps.set(slot, bootstrap_cap)?;

        let mut frame = TrapFrame::default();
        frame.sepc = entry_pc.raw();

        let (child_as, stack_top) = match address_space {
            Some(handle) => {
                let sp = stack_sp.ok_or(SpawnError::InvalidStackPointer)?;
                (handle, sp)
            }
            None => {
                if stack_sp.is_some() {
                    return Err(SpawnError::InvalidStackPointer);
                }
                let handle = address_spaces.create()?;
                let top = allocate_guarded_stack(address_spaces, handle)?;
                (handle, top)
            }
        };

        frame.x[2] = stack_top.raw();
        frame.x[1] = frame.sepc;
        const SSTATUS_SPIE: usize = 1 << 5;
        const SSTATUS_SPP: usize = 1 << 8;
        const SSTATUS_SUM: usize = 1 << 18;
        frame.sstatus |= SSTATUS_SPP | SSTATUS_SPIE | SSTATUS_SUM;

        let pid = self.tasks.len() as Pid;
        let task = Task {
            pid,
            parent: Some(parent),
            state: TaskState::Running,
            exit_code: None,
            frame,
            caps: child_caps,
            address_space: Some(child_as),
            bootstrap_slot: Some(slot),
            children: Vec::new(),
        };
        self.tasks.push(task);
        if let Err(err) = address_spaces.attach(child_as, pid) {
            self.tasks.pop();
            return Err(err.into());
        }

        if let Some(parent_task) = self.tasks.get_mut(parent_index) {
            parent_task.children.push(pid);
        }

        scheduler.enqueue(pid, QosClass::PerfBurst);

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

    /// Marks the current task as exited and transitions it to the zombie state.
    pub fn exit_current(&mut self, status: i32) {
        let pid = self.current_pid() as usize;
        if let Some(task) = self.tasks.get_mut(pid) {
            task.state = TaskState::Zombie;
            task.exit_code = Some(status);
            task.caps = CapTable::default();
            task.bootstrap_slot = None;
            task.frame = TrapFrame::default();
            task.children.clear();
        }
    }

    /// Attempts to reap a zombie child belonging to the current task.
    pub fn reap_child(
        &mut self,
        target: Option<Pid>,
        address_spaces: &mut AddressSpaceManager,
    ) -> Result<(Pid, i32), WaitError> {
        let parent_pid = self.current_pid();
        let parent_index = parent_pid as usize;
        if parent_index >= self.tasks.len() {
            return Err(WaitError::NoChildren);
        }

        let children_snapshot = {
            let parent_task = self.tasks.get(parent_index).ok_or(WaitError::NoChildren)?;
            if parent_task.children.is_empty() {
                return Err(WaitError::NoChildren);
            }
            parent_task.children.clone()
        };

        let selected_pid = if let Some(pid) = target {
            if pid == parent_pid {
                return Err(WaitError::InvalidTarget);
            }
            if !children_snapshot.iter().any(|candidate| *candidate == pid) {
                return Err(WaitError::NoSuchPid);
            }
            pid
        } else {
            let mut found = None;
            for child_pid in &children_snapshot {
                if let Some(child_task) = self.tasks.get(*child_pid as usize) {
                    if child_task.state == TaskState::Zombie && child_task.exit_code.is_some() {
                        found = Some(*child_pid);
                        break;
                    }
                }
            }
            match found {
                Some(pid) => pid,
                None => return Err(WaitError::WouldBlock),
            }
        };

        let child_index = selected_pid as usize;
        if child_index >= self.tasks.len() {
            return Err(WaitError::NoSuchPid);
        }

        let status = {
            let child_task = self.tasks.get(child_index).ok_or(WaitError::NoSuchPid)?;
            if child_task.parent != Some(parent_pid) {
                return Err(WaitError::NoSuchPid);
            }
            if child_task.state != TaskState::Zombie {
                return Err(WaitError::WouldBlock);
            }
            child_task.exit_code.ok_or(WaitError::WouldBlock)?
        };

        if let Some(parent_task) = self.tasks.get_mut(parent_index) {
            parent_task.children.retain(|pid| *pid != selected_pid);
        }

        if let Some(child_task) = self.tasks.get_mut(child_index) {
            child_task.exit_code = None;
            child_task.parent = None;
            child_task.caps = CapTable::default();
            child_task.bootstrap_slot = None;
            child_task.frame = TrapFrame::default();
            child_task.children.clear();
            if let Some(handle) = child_task.address_space.take() {
                if let Err(err) = address_spaces.detach(handle, selected_pid) {
                    log_error!(target: "task", "TASK: detach failed pid={} err={:?}", selected_pid, err);
                }
            }
        }

        Ok((selected_pid, status))
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
        let entry = VirtAddr::instr_aligned(0).unwrap();
        let child = table
            .spawn(0, entry, None, None, SlotIndex(0), &mut scheduler, &mut router, &mut spaces)
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
