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

pub use crate::image_allocs::ImageAllocs;
use alloc::vec::Vec;
use core::marker::PhantomData;

use crate::{
    cap::{CapError, CapTable, CapabilityKind, Rights},
    ipc::{self, Router},
    mm::{AddressSpaceError, AddressSpaceManager, AsHandle},
    sched::{EnqueueOutcome, QosClass, Scheduler},
    trap::{TrapDomainId, TrapFrame},
    types::{SlotIndex, VirtAddr},
};

pub use crate::types::Pid;

/// Default scheduling shares (one preemption tick per slice).
pub const DEFAULT_SHARES: u16 = 100;

/// Lifecycle state of a task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskState {
    /// Task is enqueued and schedulable.
    Running,
    /// Task has not yet been enqueued — waiting for parent to call `resume`.
    /// Transitions to `Running` on the first scheduler enqueue.
    Suspended,
    /// Task has exited and can be reaped via `wait`.
    Zombie,
}

mod block_reason;
pub use block_reason::BlockReason;

#[must_use = "wake outcomes must be handled explicitly"]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WakeOutcome {
    TaskNotFound,
    TaskNotBlocked,
    Woken,
    WokenNoopSelftest,
    EnqueueRejected,
}

#[must_use = "resume outcomes must be handled explicitly"]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResumeOutcome {
    TaskNotFound,
    NotSuspended,
    Resumed,
    EnqueueRejected,
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

mod affinity;
/// TASK-0054C P4a: the state that lets `ipc_call` be committed and finished by its peer.
pub mod completion;
mod exit_reason;
mod init_caps;
pub use affinity::{admit_affinity, clamp_home_to_affinity};
pub use exit_reason::ExitReason;

/// ADR-0082: the process model — freeze, thaw, the orders a hart executes at its trap entry.
pub mod process;
#[cfg(target_os = "none")]
mod stack_pool;
/// Waking a blocked task (TASK-0054C P4c-2: the runqueue half is measured here).
mod wake;
pub use process::Pending;
/// The reap: the parent's `wait` and the release of a thread whose process is gone.
mod reap;
#[cfg(target_os = "none")]
use stack_pool::allocate_guarded_stack;

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
    /// Scheduler runqueue rejected enqueue due to bounded capacity.
    RunQueueFull,
    /// The kernel heap above its reserve cannot hold a new task — its entry and its capability
    /// table (ADR-0091): `ENOMEM` to the caller, never an allocation panic.
    OutOfMemory,
}

/// Kernel heap a new task takes: its entry (~1.3 KiB), its capability table (256 slots of
/// ~32 B), its parent's child record — rounded up (ADR-0091).
const TASK_HEAP_COST: usize = 16 * 1024;

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
        StackExhausted | RunQueueFull | OutOfMemory => SpawnFailReason::OutOfMemory,
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

/// Minimal task control block. Not `Clone`: its table holds capabilities, and a held
/// capability is never copied (RFC-0102).
pub struct Task {
    #[cfg_attr(not(test), allow(dead_code))]
    pid: Pid,
    parent: Option<Pid>,
    state: TaskState,
    /// Exit code + ADR-0056 reason — present exactly while the task is a
    /// reapable zombie, so the reason can never outlive or predate the code.
    exit_code: Option<(i32, ExitReason)>,
    frame: TrapFrame,
    caps: CapTable,
    trap_domain: TrapDomainId,
    qos: QosClass,
    blocked: bool,
    block_reason: Option<BlockReason>,
    call_state: Option<completion::CallState>, // TASK-0054C P4a
    /// Handle referencing the address space bound to this task.
    pub address_space: Option<AsHandle>,
    /// Optional user-mode guard metadata (diagnostics only; RFC-0004 Phase 1).
    user_guard_info: Option<UserGuardInfo>,
    /// Kernel-derived stable identity for this task's service image (BootstrapInfo v2).
    ///
    /// This is set by `exec_v2` (init-lite passes the service name; kernel derives a stable id).
    service_id: u64,
    /// CPU where this task was spawned (used by wake() to enqueue on the correct
    /// per-CPU runqueue, avoiding cross-CPU migration during IPC wakeups).
    #[cfg_attr(not(test), allow(dead_code))]
    home_cpu: crate::types::CpuId,
    /// B (TASK-0042, RFC-0118): the CPUs this task may run on. Default: every CPU
    /// the kernel runs. Enforced at wake/resume (home clamp) and in the steal
    /// predicate; admitted at the syscall boundary (`admit_affinity`).
    affinity: nexus_sched::CpuSet,
    /// B (TASK-0042): scheduling shares within the QoS class, clamped to
    /// [1, 1000]; 100 = default (one preemption tick per slice).
    shares: u16,
    /// Last spawn failure reason recorded for this task (RFC-0013 boot gates).
    last_spawn_fail_reason: Option<SpawnFailReason>,
    bootstrap_slot: Option<usize>,
    children: Vec<Pid>,
    /// Arena ranges backing this task's image (`exec` records; teardown frees).
    image_allocs: ImageAllocs,
    /// The VMOs this task's capabilities named when it exited (TASK-0333 6a, RFC-0100 §1):
    /// the zombie carries them to its reap, where the ones nobody else names or maps are freed
    /// together with what its address space mapped.
    orphan_vmos: Vec<u32>,
    /// The endpoint this task bound for its children's deaths (TASK-0333 6a, like a timer's
    /// notify endpoint): every child's exit funnel writes one `OP_CHILD_EXITED` frame into it.
    exit_notify: Option<crate::ipc::EndpointId>,
    /// TASK-0333 6d (ADR-0082): spawned INTO its parent's own address space — a thread of the
    /// process whose leader created the space; it dies and freezes with the process.
    is_thread: bool,
    /// ADR-0082: the parent may `task_kill` it and the kernel's last resort may take it. Set by
    /// the parent at `exec_vmo`, never by the task about itself.
    killable: bool,
    /// ADR-0082: frozen by its parent (`task_suspend`) — `Suspended` until the thaw, a wake only
    /// clears its block. `frozen_at` is the proof line's clock.
    frozen: bool,
    frozen_at: u64,
    /// ADR-0082: an order for a task running on another hart, executed at its next trap entry.
    pending: Option<process::Pending>,
    /// ADR-0082: how many threads this leader's exit took (the reap's proof line).
    threads_taken: u8,
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
        let caps = CapTable::new_for(Pid::KERNEL.as_raw());
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

        Self {
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
            call_state: None,
            address_space: None,
            user_guard_info: None,
            service_id: 0,
            home_cpu: crate::types::CpuId::BOOT,
            affinity: crate::topology::runs(),
            shares: DEFAULT_SHARES,
            last_spawn_fail_reason: None,
            bootstrap_slot: None,
            children: Vec::new(),
            image_allocs: ImageAllocs::new(),
            orphan_vmos: Vec::new(),
            exit_notify: None,
            is_thread: false,
            killable: false,
            frozen: false,
            frozen_at: 0,
            pending: None,
            threads_taken: 0,
        }
    }

    /// ADR-0082: a thread of its process (spawned into the parent's own address space).
    pub fn is_thread(&self) -> bool {
        self.is_thread
    }

    /// ADR-0082: whether the parent may kill this task (and the last resort take it).
    pub fn killable(&self) -> bool {
        self.killable
    }

    /// ADR-0082: set once by the parent at `exec_vmo`.
    pub fn set_killable(&mut self, killable: bool) {
        self.killable = killable;
    }

    /// ADR-0082: frozen by its parent, waiting for the thaw.
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// ADR-0082: the order left for this task by another hart, if any.
    pub fn pending(&self) -> Option<process::Pending> {
        self.pending
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

    /// The endpoint this task's children's deaths are written to, if it bound one.
    pub fn exit_notify(&self) -> Option<crate::ipc::EndpointId> {
        self.exit_notify
    }

    /// Binds (or clears) the child-death endpoint.
    pub fn set_exit_notify(&mut self, endpoint: Option<crate::ipc::EndpointId>) {
        self.exit_notify = endpoint;
    }

    /// Returns the lifecycle state of the task.
    pub fn state(&self) -> TaskState {
        self.state
    }

    /// Returns the stored exit code, if any.
    pub fn exit_code(&self) -> Option<i32> {
        self.exit_code.map(|(code, _reason)| code)
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

    /// Sets the scheduler QoS class for this task.
    /// The task's home CPU (scheduling anchor).
    pub fn home_cpu(&self) -> crate::types::CpuId {
        self.home_cpu
    }

    /// B (TASK-0042): the CPUs this task may run on (bit N = logical CPU N).
    pub fn affinity(&self) -> nexus_sched::CpuSet {
        self.affinity
    }

    /// Sets the CPU set (the caller admitted it: `admit_affinity`).
    pub fn set_affinity(&mut self, set: nexus_sched::CpuSet) {
        self.affinity = set;
    }

    /// Whether this task may run on `cpu`.
    pub fn affinity_allows(&self, cpu: crate::types::CpuId) -> bool {
        self.affinity.contains(cpu.as_index())
    }

    /// B (TASK-0042): scheduling shares within the QoS class [1, 1000].
    pub fn shares(&self) -> u16 {
        self.shares
    }

    /// Sets the shares value (clamped to [1, 1000] by the caller).
    pub fn set_shares(&mut self, shares: u16) {
        self.shares = shares;
    }

    pub fn set_qos(&mut self, qos: QosClass) {
        self.qos = qos;
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
    /// A3: per-CPU current task with implicit current-CPU addressing — each
    /// hart's syscall path sees ITS running task, not the last one any hart
    /// dispatched. Access is serialized by the BKL (`trap::KernelGuard`).
    current: alloc::boxed::Box<[Pid]>,
    _not_send_sync: PhantomData<*mut ()>,
}
static_assertions::assert_not_impl_any!(TaskTable: Send, Sync);

impl TaskTable {
    /// Creates a new table seeded with the bootstrap task (PID 0).
    pub fn new() -> Self {
        let tasks_vec: Vec<Task> = alloc::vec![Task::bootstrap()];
        Self {
            tasks: tasks_vec,
            current: alloc::vec![Pid::KERNEL; crate::topology::hart_count()].into_boxed_slice(),
            _not_send_sync: PhantomData,
        }
    }

    #[inline]
    fn current_cpu_index() -> usize {
        let idx = crate::smp::cpu_current_id().as_index();
        idx * (idx < crate::topology::hart_count()) as usize
    }

    /// Returns the PID of the task currently running on THIS hart.
    pub fn current_pid(&self) -> Pid {
        self.current[Self::current_cpu_index()]
    }

    /// Changes the task currently running on THIS hart.
    pub fn set_current(&mut self, pid: Pid) {
        // ADR-0082: a frozen task is never dispatched — counted, the freeze proof's invariant.
        if self.tasks.get(pid.as_index()).is_some_and(|t| t.frozen) {
            process::note_frozen_ran(pid);
        }
        self.current[Self::current_cpu_index()] = pid;
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
        let (parent_affinity, parent_shares) = self
            .tasks
            .get(parent_index)
            .map(|p| (p.affinity, p.shares))
            .unwrap_or((crate::topology::runs(), DEFAULT_SHARES));
        let task = Task {
            pid,
            parent: Some(parent),
            state: TaskState::Running,
            exit_code: None,
            frame: TrapFrame::default(),
            caps: CapTable::new_for(pid.as_raw()),
            trap_domain: parent_domain,
            qos: QosClass::PerfBurst,
            // Selftest dummy tasks are used as bookkeeping endpoints for block/wake semantics.
            // They must never be scheduled to execute (their frame is intentionally minimal).
            // Only `wake()` should enqueue them when a selftest needs runnable state.
            blocked: true,
            block_reason: None,
            call_state: None,
            address_space: None,
            user_guard_info: None,
            service_id: 0,
            // A4 initial placement: deterministic round-robin across online
            // CPUs (pre-runtime-ready: spawning hart). See smp::assign_spawn_cpu.
            home_cpu: crate::smp::assign_spawn_cpu(),
            // B: inherit scheduling attributes from the parent (reset-on-exec
            // happens naturally: execd spawns fresh children).
            affinity: parent_affinity,
            shares: parent_shares,
            last_spawn_fail_reason: None,
            bootstrap_slot: None,
            children: Vec::new(),
            image_allocs: ImageAllocs::new(),
            orphan_vmos: Vec::new(),
            exit_notify: None,
            is_thread: false,
            killable: false,
            frozen: false,
            frozen_at: 0,
            pending: None,
            threads_taken: 0,
        };
        // B: the round-robin placement must respect the inherited mask.
        let clamped =
            clamp_home_to_affinity(task.affinity, task.home_cpu, crate::smp::cpu_online());
        let mut task = task;
        task.home_cpu = clamped;
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
        &self.tasks[self.current_pid().as_index()]
    }

    /// Returns a mutable reference to the current task.
    pub fn current_task_mut(&mut self) -> &mut Task {
        let idx = self.current_pid().as_index();
        &mut self.tasks[idx]
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
        if self.tasks.get(parent_index).is_none() {
            return Err(SpawnError::InvalidParent);
        }
        // ADR-0091: the task's heap is taken up front, above the reserve — a spawn that would
        // eat into it is refused here, never an allocation panic further down.
        if !crate::kheap::admits(TASK_HEAP_COST) || self.tasks.try_reserve(1).is_err() {
            return Err(SpawnError::OutOfMemory);
        }
        let parent_task = self.tasks.get(parent_index).ok_or(SpawnError::InvalidParent)?;

        ensure_entry_in_kernel_text(entry_pc, address_space)?;

        let parent_domain = parent_task.trap_domain();
        let parent_address_space = parent_task.address_space;

        let slot = bootstrap_slot.0;
        let bootstrap_cap = parent_task.caps.get(slot)?;
        match bootstrap_cap.kind {
            CapabilityKind::Endpoint(_) => {}
            _ => return Err(SpawnError::BootstrapNotEndpoint),
        }

        let pid = Pid::from_raw(self.tasks.len() as u32);
        let mut child_caps = CapTable::new_for(pid.as_raw());
        child_caps.set(slot, parent_task.caps.dup(slot, bootstrap_cap.rights)?)?;

        // The kernel's direct userspace child (init) gets its fixed slots — the endpoint
        // factory, the device tree, the console ring — from `init_caps` (the ABI's numbers).
        if parent == Pid::KERNEL && address_space.is_some() {
            init_caps::inject(&parent_task.caps, &mut child_caps);
        }

        let mut frame = TrapFrame { sepc: entry_pc.raw(), ..TrapFrame::default() };

        let mut stack_block = None;
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
                let (top, block) = allocate_guarded_stack(address_spaces, handle)?;
                stack_block = Some(block);
                (handle, top)
            }
        };

        // Seed minimal user frame: sp from caller, gp passed in, ra cleared so
        // accidental returns do not jump into arbitrary memory.
        frame.x[2] = stack_top.raw();
        frame.x[1] = 0;
        frame.x[3] = global_pointer;

        log_debug!(
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

        // B: inherit scheduling attributes from the parent (reset-on-exec
        // happens naturally: execd spawns fresh children).
        let (parent_affinity, parent_shares) = self
            .tasks
            .get(parent_index)
            .map(|p| (p.affinity, p.shares))
            .unwrap_or((crate::topology::runs(), DEFAULT_SHARES));
        let mut task = Task {
            pid,
            parent: Some(parent),
            state: TaskState::Suspended,
            exit_code: None,
            frame,
            caps: child_caps,
            trap_domain: parent_domain,
            qos: QosClass::Normal,
            blocked: false,
            block_reason: None,
            call_state: None,
            address_space: Some(child_as),
            user_guard_info: None,
            service_id: 0,
            // A4 initial placement: deterministic round-robin across online
            // CPUs (pre-runtime-ready: spawning hart). See smp::assign_spawn_cpu.
            home_cpu: crate::smp::assign_spawn_cpu(),
            // B: inherit scheduling attributes from the parent (reset-on-exec
            // happens naturally: execd spawns fresh children).
            affinity: parent_affinity,
            shares: parent_shares,
            last_spawn_fail_reason: None,
            bootstrap_slot: Some(slot),
            children: Vec::new(),
            image_allocs: ImageAllocs::new(),
            orphan_vmos: Vec::new(),
            exit_notify: None,
            // ADR-0082: spawned into the parent's own space = a thread of the parent's process.
            is_thread: address_space.is_some() && address_space == parent_address_space,
            killable: false,
            frozen: false,
            frozen_at: 0,
            pending: None,
            threads_taken: 0,
        };
        if let Some(block) = stack_block {
            task.image_allocs.push(block);
        }
        // B: the round-robin placement must respect the inherited mask.
        task.home_cpu =
            clamp_home_to_affinity(task.affinity, task.home_cpu, crate::smp::cpu_online());
        self.tasks.push(task);
        if let Err(err) = address_spaces.attach(child_as, pid) {
            self.tasks.pop();
            return Err(err.into());
        }

        if let Some(parent_task) = self.tasks.get_mut(parent_index) {
            parent_task.children.push(pid);
        }

        // Only enqueue if the task is not suspended (parent will resume later).
        let suspended = self.tasks.last().is_some_and(|t| t.state == TaskState::Suspended);
        let child_home = self.tasks.last().map_or(crate::types::CpuId::BOOT, |t| t.home_cpu);
        if !suspended {
            let outcome = scheduler.enqueue_on_cpu(child_home, pid, QosClass::Normal);
            if matches!(outcome, EnqueueOutcome::Enqueued)
                && child_home != crate::smp::cpu_current_id()
            {
                // A4: kick the target hart out of WFI for prompt first dispatch.
                let _ = crate::smp::request_resched(child_home);
            }
            if matches!(outcome, EnqueueOutcome::Rejected(_)) {
                if let Some(parent_task) = self.tasks.get_mut(parent_index) {
                    parent_task.children.retain(|child_pid| *child_pid != pid);
                }
                let _ = address_spaces.detach(child_as, pid);
                let _ = address_spaces.destroy(child_as);
                self.tasks.pop();
                return Err(SpawnError::RunQueueFull);
            }
        }

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
        let parent_task = self.tasks.get(parent.as_index()).ok_or(TransferError::InvalidParent)?;
        let derived = parent_task.caps.dup(parent_slot, rights)?;
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

    /// Records the arena ranges backing `pid`'s process image (`exec`).
    pub fn set_image_allocs(&mut self, pid: Pid, allocs: ImageAllocs) {
        if let Some(task) = self.tasks.get_mut(pid.as_index()) {
            task.image_allocs = allocs;
        }
    }

    /// Exits `pid` (→ Zombie) and RETURNS its process-image ranges: the zombie keeps only its
    /// exit code, so its image memory is the caller's to hand back to the arena (see
    /// `syscall::api::task_image`). The current task on a voluntary exit; a task no hart runs
    /// on a kill (ADR-0082).
    #[must_use = "a dropped image record leaks the task's arena memory"]
    pub fn exit_task(&mut self, pid: Pid, status: i32, reason: ExitReason) -> ImageAllocs {
        let Some(task) = self.tasks.get_mut(pid.as_index()) else {
            return ImageAllocs::new();
        };
        task.state = TaskState::Zombie;
        task.frozen = false;
        task.pending = None;
        task.clear_blocked();
        task.exit_code = Some((status, reason));
        // TASK-0333 6f-3: a half console line the task left is ended at its death.
        crate::hal::console_line::task_exited(pid.as_raw());
        // What the table named is remembered for the reap (RFC-0100 §1): the caps go now, the
        // objects nobody else names or maps go with the address space.
        task.orphan_vmos.clear();
        task.caps.vmo_ids(&mut task.orphan_vmos);
        task.caps = CapTable::default();
        task.bootstrap_slot = None;
        task.frame = TrapFrame::default();
        task.children.clear();
        task.image_allocs.take()
    }

    /// Blocks the current task and removes it from the scheduler (not runnable).
    pub fn block_current(&mut self, reason: BlockReason, scheduler: &mut Scheduler) {
        let pid = self.current_pid();
        // Arm the O(1) deadline-sweep gate BEFORE the task can expire: the
        // sweep skips its O(tasks) scan while `now` is below this bound, so a
        // deadline that never lowered it would be missed
        // (`trap::budgets::NEXT_DEADLINE_NS`).
        #[cfg(all(target_arch = "riscv64", target_os = "none"))]
        match reason {
            BlockReason::IpcRecv { deadline_ns, .. }
            | BlockReason::IpcSend { deadline_ns, .. }
            | BlockReason::Waitset { deadline_ns, .. }
            | BlockReason::Fence { deadline_ns, .. } => {
                crate::trap::budgets::note_block_deadline(deadline_ns);
            }
            _ => {}
        }
        if let Some(task) = self.task_mut(pid) {
            task.set_blocked(reason);
        }
        // Ensure the scheduler does not keep queued references to this PID.
        scheduler.purge(pid);
        scheduler.finish_current();
    }

    /// Explicit migration (A8 work stealing, a pin): rebinds a task's home CPU; the device
    /// lines it bound follow it (RFC-0118).
    pub fn set_home_cpu(&mut self, pid: Pid, cpu: crate::types::CpuId) {
        if let Some(task) = self.task_mut(pid) {
            task.home_cpu = cpu;
            crate::irq::rehome(pid, cpu);
        }
    }
}

#[cfg(all(test, target_arch = "riscv64", target_os = "none"))]
mod tests;
