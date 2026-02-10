// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Task table and lifecycle helpers for the NEURON kernel
//! OWNERS: @kernel-sched-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU smoke + kernel selftests (spawn/exit/wait)
//! PUBLIC API: TaskTable (spawn/exit/wait), Pid, TaskState
//! DEPENDS_ON: ipc::Router, mm::AddressSpaceManager, sched::Scheduler, types::{Pid,SlotIndex,VirtAddr}
//! INVARIANTS: Kernel text entry validation; guard-paged user stacks; capability rights respected
//! ADR: docs/adr/0001-runtime-roles-and-boundaries.md

extern crate alloc;

use alloc::vec::Vec;
use core::marker::PhantomData;

use crate::{
    cap::{CapError, CapTable, Capability, CapabilityKind, Rights},
    ipc::{self, Router},
    mm::{AddressSpaceError, AddressSpaceManager, AsHandle, PageFlags, PAGE_SIZE},
    sched::{QosClass, Scheduler},
    trap::{TrapDomainId, TrapFrame},
    types::{SlotIndex, VirtAddr},
};
use spin::Mutex;

pub use crate::types::Pid;

/// Lifecycle state of a task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskState {
    Running,
    Zombie,
}

/// Scheduler-visible blocking reason for a task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockReason {
    IpcRecv { endpoint: ipc::EndpointId, deadline_ns: u64 },
    IpcSend { endpoint: ipc::EndpointId, deadline_ns: u64 },
    WaitChild { target: Option<Pid> },
}

/// Errors returned when waiting for child processes.
#[must_use = "wait errors must be handled explicitly"]
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
        // Robust bring-up: if `.data` initializers are unavailable (or if this static lives in
        // a NOLOAD region), `cursor` may be zero. Treat zero as "uninitialized" and seed it from
        // the compile-time limit.
        if self.cursor == 0 {
            self.cursor = STACK_POOL_LIMIT;
        }
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
    // RFC-0004: zero newly allocated stack pages so no stale bytes leak into user space.
    // This relies on the kernel identity-mapping `STACK_POOL_BASE..STACK_POOL_LIMIT`.
    unsafe {
        core::ptr::write_bytes(phys_base as *mut u8, 0, STACK_PAGES * PAGE_SIZE);
    }
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

fn ensure_entry_in_kernel_text(
    entry: VirtAddr,
    address_space: Option<AsHandle>,
) -> Result<(), SpawnError> {
    // Skip validation for user-mode processes (when custom AS is provided)
    if address_space.is_some() {
        return Ok(());
    }

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

/// Spawn failure taxonomy used by boot gates (RFC-0013).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpawnFailReason {
    /// Unknown/unspecified failure reason.
    Unknown,
    /// Allocation or memory exhaustion.
    OutOfMemory,
    /// Capability table exhausted.
    CapTableFull,
    /// IPC endpoint quota exhausted.
    EndpointQuota,
    /// Address space mapping or handle error.
    MapFailed,
    /// Invalid or malformed payload/arguments.
    InvalidPayload,
    /// Spawn denied by policy (if gating applies at the spawn boundary).
    DeniedByPolicy,
}

impl SpawnFailReason {
    pub fn as_u8(self) -> u8 {
        match self {
            Self::Unknown => 0,
            Self::OutOfMemory => 1,
            Self::CapTableFull => 2,
            Self::EndpointQuota => 3,
            Self::MapFailed => 4,
            Self::InvalidPayload => 5,
            Self::DeniedByPolicy => 6,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::OutOfMemory => "oom",
            Self::CapTableFull => "cap-table-full",
            Self::EndpointQuota => "endpoint-quota",
            Self::MapFailed => "map-failed",
            Self::InvalidPayload => "invalid-payload",
            Self::DeniedByPolicy => "denied-by-policy",
        }
    }
}

/// Error returned when spawning a new task.
#[must_use = "spawn errors must be handled explicitly"]
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

/// Maps spawn errors into the stable boot-gate reason taxonomy (RFC-0013).
pub fn spawn_fail_reason(err: &SpawnError) -> SpawnFailReason {
    use SpawnError::*;
    match err {
        InvalidParent | InvalidEntryPoint | InvalidStackPointer | BootstrapNotEndpoint => {
            SpawnFailReason::InvalidPayload
        }
        Capability(CapError::NoSpace) => SpawnFailReason::CapTableFull,
        Capability(_) => SpawnFailReason::InvalidPayload,
        Ipc(ipc::IpcError::NoSpace) => SpawnFailReason::EndpointQuota,
        Ipc(_) => SpawnFailReason::InvalidPayload,
        AddressSpace(AddressSpaceError::AsidExhausted) => SpawnFailReason::MapFailed,
        AddressSpace(AddressSpaceError::Mapping(_)) => SpawnFailReason::MapFailed,
        AddressSpace(_) => SpawnFailReason::InvalidPayload,
        StackExhausted => SpawnFailReason::OutOfMemory,
    }
}

/// Error returned when transferring capabilities between tasks.
#[must_use = "transfer errors must be handled explicitly"]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferError {
    /// Parent PID does not exist.
    InvalidParent,
    /// Child PID does not exist.
    InvalidChild,
    /// Capability operation failed.
    Capability(CapError),
}

/// Internal transfer intent used to centralize capability transfer semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TransferMode {
    CopyAllocate,
    CopyToSlot(usize),
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
    trap_domain: TrapDomainId,
    qos: QosClass,
    blocked: bool,
    block_reason: Option<BlockReason>,
    /// Handle referencing the address space bound to this task.
    pub address_space: Option<AsHandle>,
    /// Optional user-mode guard metadata (diagnostics only; RFC-0004 Phase 1).
    user_guard_info: Option<UserGuardInfo>,
    /// Kernel-derived stable identity for this task's service image (BootstrapInfo v2).
    ///
    /// This is set by `exec_v2` (init-lite passes the service name; kernel derives a stable id).
    service_id: u64,
    /// Last spawn failure reason recorded for this task (RFC-0013 boot gates).
    last_spawn_fail_reason: Option<SpawnFailReason>,
    bootstrap_slot: Option<usize>,
    children: Vec<Pid>,
}

/// Minimal guard metadata used by the trap handler to attribute user page faults.
///
/// This is *diagnostic only* and must not be relied upon for correctness/security decisions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UserGuardInfo {
    /// Base VA of the stack guard page (unmapped).
    pub stack_guard_va: usize,
    /// Base VA of the bootstrap-info guard page (unmapped).
    pub info_guard_va: Option<usize>,
}

impl Task {
    fn bootstrap() -> Self {
        let caps = CapTable::new();
        // Avoid Default impl to minimize any unexpected code paths during bring-up.
        //
        // CRITICAL: PID 0 is not a real user task. Ensure it is marked as S-mode (SPP=1) so the
        // idle loop never attempts to context-switch into it as a userspace process.
        const SSTATUS_SPIE: usize = 1 << 5;
        const SSTATUS_SPP: usize = 1 << 8;
        const SSTATUS_SUM: usize = 1 << 18;
        let zero_frame = TrapFrame {
            x: [0; 32],
            sepc: 0,
            sstatus: SSTATUS_SPP | SSTATUS_SPIE | SSTATUS_SUM,
            scause: 0,
            stval: 0,
        };
        let t = Self {
            pid: Pid::KERNEL,
            parent: None,
            state: TaskState::Running,
            exit_code: None,
            frame: zero_frame,
            caps,
            trap_domain: TrapDomainId::default(),
            qos: QosClass::PerfBurst,
            blocked: false,
            block_reason: None,
            address_space: None,
            user_guard_info: None,
            service_id: 0,
            last_spawn_fail_reason: None,
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

    /// Returns user guard metadata, if any (diagnostics).
    pub fn user_guard_info(&self) -> Option<UserGuardInfo> {
        self.user_guard_info
    }

    /// Sets user guard metadata (diagnostics).
    pub fn set_user_guard_info(&mut self, info: UserGuardInfo) {
        self.user_guard_info = Some(info);
    }

    /// Returns the kernel-derived service identity for this task.
    pub fn service_id(&self) -> u64 {
        self.service_id
    }

    /// Sets the kernel-derived service identity for this task.
    pub fn set_service_id(&mut self, id: u64) {
        self.service_id = id;
    }

    /// Returns the trap domain associated with this task.
    pub fn trap_domain(&self) -> TrapDomainId {
        self.trap_domain
    }

    /// Sets the trap domain for this task.
    pub fn set_trap_domain(&mut self, domain: TrapDomainId) {
        self.trap_domain = domain;
    }

    /// Returns the slot that seeded the bootstrap endpoint, if any.
    pub fn bootstrap_slot(&self) -> Option<usize> {
        self.bootstrap_slot
    }

    /// Returns the QoS class used by the scheduler.
    pub fn qos(&self) -> QosClass {
        self.qos
    }

    pub fn set_last_spawn_fail_reason(&mut self, reason: SpawnFailReason) {
        self.last_spawn_fail_reason = Some(reason);
    }

    pub fn take_last_spawn_fail_reason(&mut self) -> Option<SpawnFailReason> {
        self.last_spawn_fail_reason.take()
    }

    /// Returns whether this task is currently blocked (not runnable).
    pub fn is_blocked(&self) -> bool {
        self.blocked
    }

    /// Returns the current block reason, if any.
    pub fn block_reason(&self) -> Option<BlockReason> {
        self.block_reason
    }

    fn set_blocked(&mut self, reason: BlockReason) {
        self.blocked = true;
        self.block_reason = Some(reason);
    }

    fn clear_blocked(&mut self) {
        self.blocked = false;
        self.block_reason = None;
    }
}

/// Kernel task table managing task control blocks.
pub struct TaskTable {
    tasks: Vec<Task>,
    current: Pid,
    // Pre-SMP contract: task table stays in the single kernel execution context.
    _not_send_sync: PhantomData<*mut ()>,
}

impl TaskTable {
    /// Creates a new table seeded with the bootstrap task (PID 0).
    pub fn new() -> Self {
        let mut tasks_vec: Vec<Task> = Vec::new();
        tasks_vec.push(Task::bootstrap());
        Self { tasks: tasks_vec, current: Pid::KERNEL, _not_send_sync: PhantomData }
    }

    /// Returns the PID of the currently running task.
    pub fn current_pid(&self) -> Pid {
        self.current
    }

    /// Changes the currently running task.
    pub fn set_current(&mut self, pid: Pid) {
        self.current = pid;
    }

    /// Selftest helper: create a minimal task entry without allocating a new address space or stack.
    ///
    /// This is intentionally *not* a general spawn primitive; it exists so kernel selftests can
    /// exercise block/wake scheduling logic without perturbing memory pressure (e.g. exec PT_LOAD).
    pub fn selftest_create_dummy_task(&mut self, parent: Pid, scheduler: &mut Scheduler) -> Pid {
        let parent_index = parent.as_index();
        let parent_task =
            self.tasks.get(parent_index).expect("selftest_create_dummy_task: invalid parent");
        let parent_domain = parent_task.trap_domain();

        let pid = Pid::from_raw(self.tasks.len() as u32);
        let task = Task {
            pid,
            parent: Some(parent),
            state: TaskState::Running,
            exit_code: None,
            frame: TrapFrame::default(),
            caps: CapTable::new(),
            trap_domain: parent_domain,
            qos: QosClass::PerfBurst,
            // Selftest dummy tasks are used as bookkeeping endpoints for block/wake semantics.
            // They must never be scheduled to execute (their frame is intentionally minimal).
            // Only `wake()` should enqueue them when a selftest needs runnable state.
            blocked: true,
            block_reason: None,
            address_space: None,
            user_guard_info: None,
            service_id: 0,
            last_spawn_fail_reason: None,
            bootstrap_slot: None,
            children: Vec::new(),
        };
        self.tasks.push(task);
        if let Some(parent_task) = self.tasks.get_mut(parent_index) {
            parent_task.children.push(pid);
        }
        // Do not enqueue by default: prevents accidental scheduling of a dummy task and
        // avoids stray runnable PIDs after kernel selftests.
        let _ = scheduler;
        pid
    }

    /// Returns the kernel-derived service identity for the currently running task.
    pub fn current_service_id(&self) -> u64 {
        self.current_task().service_id()
    }

    /// Returns the number of allocated task slots (PIDs).
    pub fn len(&self) -> usize {
        self.tasks.len()
    }

    /// Returns a mutable reference to the bootstrap task (PID 0).
    pub fn bootstrap_mut(&mut self) -> &mut Task {
        &mut self.tasks[0]
    }

    /// Returns a shared reference to the current task.
    pub fn current_task(&self) -> &Task {
        &self.tasks[self.current.as_index()]
    }

    /// Returns a mutable reference to the current task.
    pub fn current_task_mut(&mut self) -> &mut Task {
        &mut self.tasks[self.current.as_index()]
    }

    /// Returns a shared reference to a task by PID.
    pub fn task(&self, pid: Pid) -> Option<&Task> {
        self.tasks.get(pid.as_index())
    }

    /// Returns the capability table of the current task.
    pub fn current_caps_mut(&mut self) -> &mut CapTable {
        self.current_task_mut().caps_mut()
    }

    /// Returns a shared reference to the capability table of `pid`.
    pub fn caps_of(&self, pid: Pid) -> Option<&CapTable> {
        self.tasks.get(pid.as_index()).map(|task| &task.caps)
    }

    /// Returns a mutable reference to the capability table of `pid`.
    pub fn caps_of_mut(&mut self, pid: Pid) -> Option<&mut CapTable> {
        self.tasks.get_mut(pid.as_index()).map(|task| task.caps_mut())
    }

    /// Returns a mutable reference to a task by PID.
    pub fn task_mut(&mut self, pid: Pid) -> Option<&mut Task> {
        self.tasks.get_mut(pid.as_index())
    }

    pub fn set_last_spawn_fail_reason(&mut self, pid: Pid, reason: SpawnFailReason) {
        if let Some(task) = self.task_mut(pid) {
            task.set_last_spawn_fail_reason(reason);
        }
    }

    pub fn take_last_spawn_fail_reason(&mut self, pid: Pid) -> Option<SpawnFailReason> {
        self.task_mut(pid).and_then(|task| task.take_last_spawn_fail_reason())
    }

    /// Spawns a child task that temporarily shares its parent's address space.
    #[inline(always)]
    pub fn spawn(
        &mut self,
        parent: Pid,
        entry_pc: VirtAddr,
        stack_sp: Option<VirtAddr>,
        address_space: Option<AsHandle>,
        global_pointer: usize,
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
            global_pointer,
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
        global_pointer: usize,
        bootstrap_slot: SlotIndex,
        scheduler: &mut Scheduler,
        _router: &mut Router,
        address_spaces: &mut AddressSpaceManager,
    ) -> Result<Pid, SpawnError> {
        let parent_index = parent.as_index();
        let parent_task = self.tasks.get(parent_index).ok_or(SpawnError::InvalidParent)?;

        ensure_entry_in_kernel_text(entry_pc, address_space)?;

        let parent_domain = parent_task.trap_domain();

        let slot = bootstrap_slot.0;
        let bootstrap_cap = parent_task.caps.get(slot)?;
        match bootstrap_cap.kind {
            CapabilityKind::Endpoint(_) => {}
            _ => return Err(SpawnError::BootstrapNotEndpoint),
        }

        let mut child_caps = CapTable::new();
        child_caps.set(slot, bootstrap_cap)?;

        // RFC-0005 Phase 2 hardening: endpoint creation authority is held via an explicit
        // EndpointFactory capability. During bring-up, the bootstrap task (PID 0) carries this cap
        // in a fixed slot (2) and we inject a derived copy into its direct userspace child (init-lite).
        //
        // This avoids brittle PID/parent gating and avoids relying on external helpers to do
        // cap_transfer at exactly the right time during boot.
        if parent == Pid::KERNEL && address_space.is_some() {
            const FACTORY_PARENT_SLOT: usize = 2;
            const FACTORY_CHILD_SLOT: usize = 1;
            if let Ok(factory_cap) = parent_task.caps.get(FACTORY_PARENT_SLOT) {
                if factory_cap.kind == CapabilityKind::EndpointFactory
                    && factory_cap.rights.contains(Rights::MANAGE)
                {
                    // Deterministic bring-up: init-lite expects the EndpointFactory in slot 1.
                    // Slot 0 is already populated by the bootstrap endpoint capability.
                    let _ = child_caps.set(
                        FACTORY_CHILD_SLOT,
                        Capability {
                            kind: CapabilityKind::EndpointFactory,
                            rights: Rights::MANAGE,
                        },
                    );
                }
            }
        }

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

        // Seed minimal user frame: sp from caller, gp passed in, ra cleared so
        // accidental returns do not jump into arbitrary memory.
        frame.x[2] = stack_top.raw();
        frame.x[1] = 0;
        frame.x[3] = global_pointer;

        // Debug: verify SP was set
        log_info!(
            target: "task",
            "SPAWN-FRAME: sepc=0x{:x} sp(x2)=0x{:x} gp(x3)=0x{:x}",
            frame.sepc,
            frame.x[2],
            frame.x[3]
        );
        const SSTATUS_SPIE: usize = 1 << 5;
        const SSTATUS_SPP: usize = 1 << 8;
        const SSTATUS_SUM: usize = 1 << 18;

        // For user-mode processes (separate AS), clear SPP to run in U-mode
        // For kernel tasks (shared AS), set SPP to run in S-mode
        if address_space.is_some() {
            // Enable interrupts-on-return and permit S-mode to access user memory (SUM)
            // so that trap entry can save the frame on the user stack until sscratch swap is added.
            frame.sstatus |= SSTATUS_SPIE | SSTATUS_SUM; // U-mode task (SPP=0)
        } else {
            frame.sstatus |= SSTATUS_SPP | SSTATUS_SPIE | SSTATUS_SUM; // S-mode task
        }

        let pid = Pid::from_raw(self.tasks.len() as u32);
        let task = Task {
            pid,
            parent: Some(parent),
            state: TaskState::Running,
            exit_code: None,
            frame,
            caps: child_caps,
            trap_domain: parent_domain,
            qos: QosClass::PerfBurst,
            blocked: false,
            block_reason: None,
            address_space: Some(child_as),
            user_guard_info: None,
            service_id: 0,
            last_spawn_fail_reason: None,
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
        self.transfer_cap_with_mode(parent, child, parent_slot, rights, TransferMode::CopyAllocate)
    }

    fn transfer_cap_with_mode(
        &mut self,
        parent: Pid,
        child: Pid,
        parent_slot: usize,
        rights: Rights,
        mode: TransferMode,
    ) -> Result<usize, TransferError> {
        let parent_task =
            self.tasks.get(parent.as_index()).ok_or(TransferError::InvalidParent)?;
        let derived = parent_task.caps.derive(parent_slot, rights)?;
        let child_task = self.tasks.get_mut(child.as_index()).ok_or(TransferError::InvalidChild)?;
        match mode {
            TransferMode::CopyAllocate => {
                child_task.caps_mut().allocate(derived).map_err(TransferError::from)
            }
            TransferMode::CopyToSlot(child_slot) => {
                child_task
                    .caps_mut()
                    .set_if_empty(child_slot, derived)
                    .map_err(TransferError::from)?;
                Ok(child_slot)
            }
        }
    }

    /// Duplicates a capability from `parent` into `child` at an explicit slot.
    pub fn transfer_cap_to_slot(
        &mut self,
        parent: Pid,
        child: Pid,
        parent_slot: usize,
        rights: Rights,
        child_slot: usize,
    ) -> Result<(), TransferError> {
        let _ = self.transfer_cap_with_mode(
            parent,
            child,
            parent_slot,
            rights,
            TransferMode::CopyToSlot(child_slot),
        )?;
        Ok(())
    }

    /// Marks the current task as exited and transitions it to the zombie state.
    pub fn exit_current(&mut self, status: i32) {
        let pid = self.current_pid().as_index();
        if let Some(task) = self.tasks.get_mut(pid) {
            task.state = TaskState::Zombie;
            task.exit_code = Some(status);
            task.caps = CapTable::default();
            task.bootstrap_slot = None;
            task.frame = TrapFrame::default();
            task.children.clear();
        }
    }

    /// Blocks the current task and removes it from the scheduler (not runnable).
    pub fn block_current(&mut self, reason: BlockReason, scheduler: &mut Scheduler) {
        let pid = self.current_pid();
        if let Some(task) = self.task_mut(pid) {
            task.set_blocked(reason);
        }
        // Ensure the scheduler does not keep queued references to this PID.
        scheduler.purge(pid);
        scheduler.finish_current();
    }

    /// Wakes a blocked task and enqueues it for execution. Returns true if a task was woken.
    pub fn wake(&mut self, pid: Pid, scheduler: &mut Scheduler) -> bool {
        let Some(task) = self.task_mut(pid) else {
            return false;
        };
        if !task.blocked {
            return false;
        }
        task.clear_blocked();
        // Avoid duplicates; then enqueue with the stored QoS.
        scheduler.purge(pid);
        scheduler.enqueue(pid, task.qos);
        true
    }

    /// Wakes the current task's parent if it is blocked in `wait` for this child.
    pub fn wake_parent_waiter(&mut self, child: Pid, scheduler: &mut Scheduler) {
        let Some(child_task) = self.task(child) else {
            return;
        };
        let Some(parent) = child_task.parent() else {
            return;
        };
        let Some(parent_task) = self.task(parent) else {
            return;
        };
        if !parent_task.is_blocked() {
            return;
        }
        if let Some(BlockReason::WaitChild { target }) = parent_task.block_reason() {
            if target.is_none() || target == Some(child) {
                let _ = self.wake(parent, scheduler);
            }
        }
    }

    /// Attempts to reap a zombie child belonging to the current task.
    pub fn reap_child(
        &mut self,
        target: Option<Pid>,
        address_spaces: &mut AddressSpaceManager,
    ) -> Result<(Pid, i32), WaitError> {
        let parent_pid = self.current_pid();
        let parent_index = parent_pid.as_index();
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
                if let Some(child_task) = self.tasks.get(child_pid.as_index()) {
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

        let child_index = selected_pid.as_index();
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
                    log_error!(
                        target: "task",
                        "TASK: detach failed pid={} err={:?}",
                        selected_pid.as_raw(),
                        err
                    );
                }
            }
        }

        Ok((selected_pid, status))
    }
}

#[cfg(all(test, target_arch = "riscv64", target_os = "none"))]
mod tests {
    use super::*;
    use crate::cap::{CapError, Capability, CapabilityKind, Rights};
    use crate::ipc::Router;
    use crate::mm::AddressSpaceManager;
    use crate::sched::Scheduler;

    #[test]
    fn bootstrap_task_present() {
        let table = TaskTable::new();
        assert_eq!(table.current_pid(), Pid::KERNEL);
        assert_eq!(table.current_task().pid, Pid::KERNEL);
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
        spaces.attach(bootstrap_as, Pid::KERNEL).unwrap();
        table.bootstrap_mut().address_space = Some(bootstrap_as);
        let entry = VirtAddr::instr_aligned(0).unwrap();
        let child = table
            .spawn(
                Pid::KERNEL,
                entry,
                None,
                None,
                0,
                SlotIndex(0),
                &mut scheduler,
                &mut router,
                &mut spaces,
            )
            .unwrap();

        let slot = table.transfer_cap(Pid::KERNEL, child, 0, Rights::RECV).unwrap();
        assert_ne!(slot, 0);
        let cap = table.caps_of(child).unwrap().get(slot).unwrap();
        assert_eq!(cap.kind, CapabilityKind::Endpoint(0));
        assert_eq!(cap.rights, Rights::RECV);

        let err = table.transfer_cap(Pid::KERNEL, child, 0, Rights::MAP).unwrap_err();
        assert_eq!(err, TransferError::Capability(CapError::PermissionDenied));
    }
}
