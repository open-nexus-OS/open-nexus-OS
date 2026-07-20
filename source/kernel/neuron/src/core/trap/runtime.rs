// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Trap runtime state split out of the former single-file trap.rs:
//! TRAP_RUNTIME slot + KernelHandles, the kernel big lock (BKL) with the
//! KernelGuard RAII accessor (A2 lock model), install_runtime/trap-domain
//! plumbing, SBI timer utilities (timer_arm, DEFAULT_TICK_CYCLES), trap-vector
//! install and reactive timer/IPC-deadline delivery (process_expired_timers,
//! wake_expired_ipc_deadlines).
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: neuron host tests + QEMU marker gates (just test-os / ci-os-smp)
//! ADR: docs/adr/0016-kernel-libs-architecture.md

use super::*;

/// Identifier selecting a trap domain (e.g. syscall table) for a task.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct TrapDomainId(pub(crate) usize);

#[derive(Clone, Copy)]
pub(super) struct KernelHandles {
    pub(super) scheduler: NonNull<Scheduler>,
    pub(super) tasks: NonNull<task::TaskTable>,
    pub(super) router: NonNull<ipc::Router>,
    pub(super) spaces: NonNull<AddressSpaceManager>,
    pub(super) timer: *const dyn Timer,
    pub(super) hart_timers: NonNull<crate::timer::HartTimers>,
    pub(super) waitsets: NonNull<crate::waitset::WaitsetTable>,
    pub(super) fences: NonNull<crate::fence::FenceTable>,
}
// SAFETY (A2 lock model): these raw handles point at 'static kernel state.
// Every MUTABLE materialization goes through `KernelGuard`, which holds the
// KERNEL_LOCK (IRQ-safe BKL) for the full borrow duration; the only lock-free
// readers are the best-effort fault diagnostics on the way to panic
// (`runtime_kernel_handles_diagnostic`). The historic "interrupted U-mode ⇒
// unique borrow" argument is no longer load-bearing.
unsafe impl Send for KernelHandles {}
unsafe impl Sync for KernelHandles {}
static_assertions::assert_impl_all!(KernelHandles: Send, Sync);

struct TrapRuntime {
    kernel: KernelHandles,
    syscalls: NonNull<SyscallTable>,
}
unsafe impl Send for TrapRuntime {}
unsafe impl Sync for TrapRuntime {}
static_assertions::assert_impl_all!(TrapRuntime: Send, Sync);

// RFC-0003 (Phase 0): trap/syscall runtime must be deterministic and must not allocate.
// Keep runtime state in a single global slot.
static TRAP_RUNTIME: Mutex<Option<TrapRuntime>> = Mutex::new(None);

// TASK-0277 (A2a): the kernel "big lock" (BKL). Every materialization of
// `&mut` kernel subsystem references from `KernelHandles` happens under this
// lock, held for the FULL duration of use (via `KernelGuard`), not just for
// the pointer copy. Tier 1 of the lock hierarchy: BKL -> per-CPU run-queue
// locks -> leaf atomics; never acquire the BKL while holding a run-queue lock.
static KERNEL_LOCK: crate::sync::spin_irq::SpinIrqLock<()> =
    crate::sync::spin_irq::SpinIrqLock::new(());

/// RAII access to the kernel subsystems: holds the BKL until dropped and
/// materializes the `&mut` set from the installed handles.
///
/// A2 migration note: while the boot-hart gate is still in place this lock is
/// uncontended scaffolding; it becomes load-bearing when secondary harts
/// start serving syscalls (A3). Guards must NEVER be held across a context
/// switch that leaves via `sret` (A2b invariant).
pub(crate) struct KernelGuard {
    _bkl: crate::sync::spin_irq::SpinIrqGuard<'static, ()>,
    pub(super) handles: KernelHandles,
}

impl KernelGuard {
    pub(crate) fn acquire() -> Result<Self, RuntimeKernelAccessFailure> {
        // A3: every hart serves the runtime; exclusion comes from the BKL,
        // not from a boot-hart gate.
        // Bounded contention probe: long acquisition spins are the smoking
        // gun for cross-hart BKL contention inflating IPC round-trips.
        #[cfg(all(target_arch = "riscv64", target_os = "none"))]
        let t0 = riscv::register::time::read() as u64;
        // P2 soft-realtime preference: cpu0 carries the pinned display/input
        // chain (service_topology::affinity_for), so it gets RIGHT OF WAY at
        // the BKL — while cpu0 is waiting, other harts back off before
        // acquiring, letting cpu0 take the next release instead of joining a
        // convoy. Bounded (backoff limit) so background harts cannot starve.
        #[cfg(all(target_arch = "riscv64", target_os = "none"))]
        let bkl = {
            use core::sync::atomic::{AtomicBool, Ordering};
            static CPU0_WAITING: AtomicBool = AtomicBool::new(false);
            let is_cpu0 = crate::smp::cpu_current_id().is_boot();
            if is_cpu0 {
                CPU0_WAITING.store(true, Ordering::Release);
                let guard = KERNEL_LOCK.lock();
                CPU0_WAITING.store(false, Ordering::Release);
                guard
            } else {
                let mut backoff = 0u32;
                loop {
                    if CPU0_WAITING.load(Ordering::Acquire) && backoff < 200_000 {
                        backoff += 1;
                        core::hint::spin_loop();
                        continue;
                    }
                    if let Some(guard) = KERNEL_LOCK.try_lock() {
                        break guard;
                    }
                    core::hint::spin_loop();
                }
            }
        };
        #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
        let bkl = KERNEL_LOCK.lock();
        #[cfg(all(target_arch = "riscv64", target_os = "none"))]
        {
            let waited = (riscv::register::time::read() as u64).saturating_sub(t0);
            super::budgets::record_bkl_wait(waited);
            // 10 MHz mtime: 10_000 ticks = 1ms spent spinning for the lock.
            if waited > 10_000 {
                static BKL_CONTENTION_LOGGED: core::sync::atomic::AtomicUsize =
                    core::sync::atomic::AtomicUsize::new(0);
                if BKL_CONTENTION_LOGGED.fetch_add(1, core::sync::atomic::Ordering::Relaxed) < 6 {
                    log_info!(
                        target: "smp",
                        "KINIT: bkl wait {}us cpu{}",
                        waited / 10,
                        crate::smp::cpu_current_id().as_index()
                    );
                }
            }
        }
        let handles = TRAP_RUNTIME
            .lock()
            .as_ref()
            .map(|runtime| runtime.kernel)
            .ok_or(RuntimeKernelAccessFailure::NotInstalled)?;
        Ok(Self { _bkl: bkl, handles })
    }

    /// Materializes the full `&mut` set. The borrows are tied to `&mut self`,
    /// so they end before the guard (and thus the BKL) is released.
    #[allow(clippy::type_complexity)]
    pub(crate) fn parts(
        &mut self,
    ) -> (
        &mut Scheduler,
        &mut task::TaskTable,
        &mut ipc::Router,
        &mut AddressSpaceManager,
        &'static dyn Timer,
        &mut crate::timer::HartTimers,
        &mut crate::waitset::WaitsetTable,
        &mut crate::fence::FenceTable,
    ) {
        // SAFETY: the handles point at 'static kernel state installed by
        // install_runtime; the BKL is held for the lifetime of these borrows,
        // and each NonNull targets a distinct object (no aliasing among them).
        unsafe {
            (
                self.handles.scheduler.as_mut(),
                self.handles.tasks.as_mut(),
                self.handles.router.as_mut(),
                self.handles.spaces.as_mut(),
                &*self.handles.timer,
                self.handles.hart_timers.as_mut(),
                self.handles.waitsets.as_mut(),
                self.handles.fences.as_mut(),
            )
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum RuntimeKernelAccessFailure {
    NotInstalled,
}

/// Installs the runtime trap context using kernel subsystems and default syscall table.
pub fn install_runtime(
    scheduler: &mut Scheduler,
    tasks: &mut task::TaskTable,
    router: &mut ipc::Router,
    spaces: &mut AddressSpaceManager,
    timer: &'static dyn Timer,
    hart_timers: &mut crate::timer::HartTimers,
    waitsets: &mut crate::waitset::WaitsetTable,
    fences: &mut crate::fence::FenceTable,
    syscalls: &SyscallTable,
) -> TrapDomainId {
    if !crate::smp::cpu_current_id().is_boot() {
        panic!("trap runtime install must run on boot hart");
    }
    let syscalls_ptr = NonNull::new((syscalls as *const SyscallTable) as *mut SyscallTable)
        .expect("syscall table ptr");
    let runtime = TrapRuntime {
        kernel: KernelHandles {
            scheduler: NonNull::from(scheduler),
            tasks: NonNull::from(tasks),
            router: NonNull::from(router),
            spaces: NonNull::from(spaces),
            timer: timer as *const dyn Timer,
            hart_timers: NonNull::from(hart_timers),
            waitsets: NonNull::from(waitsets),
            fences: NonNull::from(fences),
        },
        syscalls: syscalls_ptr,
    };
    *TRAP_RUNTIME.lock() = Some(runtime);
    TrapDomainId::default()
}

/// Registers an additional trap domain (e.g. alternative syscall table).
#[allow(dead_code)]
pub fn register_trap_domain(syscalls: &SyscallTable) -> TrapDomainId {
    // Phase-0 runtime supports only the default domain.
    let _ = syscalls;
    TrapDomainId::default()
}

pub(crate) fn runtime_installed() -> bool {
    TRAP_RUNTIME.lock().is_some()
}

/// BKL-free handle copy for FAULT DIAGNOSTICS ONLY (read-only, best-effort).
///
/// Fault paths on the way to `panic!` may run while this hart already holds
/// the BKL (e.g. a kernel page fault inside a syscall) — acquiring
/// `KernelGuard` there would deadlock instead of printing. Never mutate
/// through these handles; every mutating path goes through `KernelGuard`.
pub(super) fn runtime_kernel_handles_diagnostic(
) -> Result<KernelHandles, RuntimeKernelAccessFailure> {
    TRAP_RUNTIME
        .lock()
        .as_ref()
        .map(|runtime| runtime.kernel)
        .ok_or(RuntimeKernelAccessFailure::NotInstalled)
}

pub(super) fn runtime_domain(id: TrapDomainId) -> Option<NonNull<SyscallTable>> {
    let _ = id;
    TRAP_RUNTIME.lock().as_ref().map(|runtime| runtime.syscalls)
}

pub(super) fn runtime_default_domain() -> TrapDomainId {
    TrapDomainId::default()
}

// ——— SBI timer utilities ———

/// Default tick in cycles (10 ms for 10 MHz mtimer on QEMU virt).
#[cfg_attr(not(all(target_arch = "riscv64", target_os = "none")), allow(dead_code))]
pub const DEFAULT_TICK_CYCLES: u64 = 100_000;

/// Arm S-mode timer via SBI for `now + delta_cycles`.
#[inline]
#[allow(dead_code)]
#[cfg(all(target_arch = "riscv64", target_os = "none", feature = "timer_irq"))]
pub fn timer_arm(delta_cycles: u64) {
    let now = riscv::register::time::read() as u64;
    sbi::set_timer(now.wrapping_add(delta_cycles));
}

#[allow(dead_code)]
#[cfg(not(all(target_arch = "riscv64", target_os = "none", feature = "timer_irq")))]
pub fn timer_arm(_delta_cycles: u64) {}

/// Install trap vector; call once during early boot on the boot hart
/// (before enabling SIE). Prepares the boot hart's HartLocal block first.
#[cfg(all(target_arch = "riscv64", target_os = "none"))]
pub unsafe fn install_trap_vector() {
    let boot = crate::types::CpuId::BOOT;
    crate::smp::hart_local_prepare(boot, crate::smp::linker_boot_stack_top());
    unsafe { install_trap_vector_for(boot) };
}

/// Install this hart's trap vector against its prepared HartLocal block:
/// adopts `tp`, points `sscratch` at the block, and writes `stvec`.
#[cfg(all(target_arch = "riscv64", target_os = "none"))]
pub unsafe fn install_trap_vector_for(cpu: crate::types::CpuId) {
    crate::smp::hart_local_adopt(cpu);
    // SAFETY: must be called early and exactly once per hart; SSCRATCH becomes well-defined.
    unsafe {
        riscv::register::sscratch::write(crate::smp::hart_local_sscratch_value(cpu));
        riscv::register::stvec::write(
            __trap_vector as usize,
            riscv::register::mtvec::TrapMode::Direct,
        );
    }
}

#[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
pub unsafe fn install_trap_vector() {}

/// Enable supervisor timer interrupts after arming the first timer.
/// Gated behind `timer_irq` feature to avoid dead_code in default builds.
#[allow(dead_code)]
#[cfg(all(target_arch = "riscv64", target_os = "none", feature = "timer_irq"))]
pub unsafe fn enable_timer_interrupts() {
    use riscv::register::{sie, sstatus};
    // SAFETY: requires trap vector installed and first timer armed.
    unsafe {
        sie::set_stimer();
        sstatus::set_sie();
    }
}

// No non-OS stub; avoid dead_code in host builds

/// Disable supervisor timer interrupts.
/// Gated behind `timer_irq` feature to avoid dead_code in default builds.
#[cfg_attr(not(test), inline)]
#[allow(dead_code)]
#[cfg(all(target_arch = "riscv64", target_os = "none", feature = "timer_irq"))]
pub unsafe fn disable_timer_interrupts() {
    use riscv::register::{sie, sstatus};
    // SAFETY: caller must ensure trap vector is installed and interrupts are masked appropriately elsewhere when needed.
    unsafe {
        sstatus::clear_sie();
        sie::clear_stimer();
    }
}

// Intentionally no non-OS stub to avoid dead_code in host builds

pub(crate) const OP_TIMER_FIRED: u8 = 0x30;

// ——— Earliest-deadline timer arming (ADR-0052) ———

/// Per-hart shadow of the armed wakeup deadline (ns; 0 = nothing armed). One
/// compare register per hart, many arming paths — the shadow lets `arm_wakeup`
/// keep the EARLIEST pending deadline armed instead of letting the last caller
/// win (windowd's 8.33ms pacer slipping to the 10ms fallback tick — the SMP
/// frame-jitter class measured by the Stage-0 slip histogram). Written only by
/// the owning hart under the BKL; Relaxed is sufficient.
static ARMED_DEADLINE_NS: [core::sync::atomic::AtomicU64; crate::smp::MAX_CPUS] =
    [const { core::sync::atomic::AtomicU64::new(0) }; crate::smp::MAX_CPUS];

fn armed_slot() -> &'static core::sync::atomic::AtomicU64 {
    let idx = crate::smp::cpu_current_id().as_index();
    &ARMED_DEADLINE_NS[if idx < crate::smp::MAX_CPUS { idx } else { 0 }]
}

/// Arm this hart's wakeup for `deadline_ns`, keeping the earliest pending
/// deadline (ADR-0052). `Timer::set_wakeup` overwrites the single per-hart
/// compare register; routing every arming path through here turns a later,
/// LONGER deadline into a no-op instead of a clobber. `deadline_ns == 0`
/// means "no deadline" and never arms.
pub(crate) fn arm_wakeup(timer: &dyn Timer, deadline_ns: u64) {
    use core::sync::atomic::Ordering;
    if deadline_ns == 0 {
        return;
    }
    let slot = armed_slot();
    let armed = slot.load(Ordering::Relaxed);
    // A shadow deadline suppresses this arm only while it is STILL PENDING.
    // Not every timer-IRQ path clears the shadow (an S-mode trap re-arms
    // without running `process_expired_timers`), so an ELAPSED shadow must
    // self-heal here — otherwise it would read as "earlier" forever and this
    // hart's timer would fall silent (observed as windowd present-NACK storms
    // right after boot).
    if armed > timer.now() && armed <= deadline_ns {
        return;
    }
    slot.store(deadline_ns, Ordering::Relaxed);
    timer.set_wakeup(deadline_ns);
}

/// Deterministic KSELFTEST probe for the ADR-0052 earliest-wins contract:
/// base arm programs, a LATER arm is a no-op, an EARLIER arm re-arms. Runs
/// against the real timer — the probe deadlines are short/far enough that the
/// extra IRQs they arm are absorbed by the normal expiry path, and the shadow
/// is left cleared so the next arm/re-arm starts fresh.
pub(crate) fn selftest_edt_probe(timer: &dyn Timer) -> bool {
    use core::sync::atomic::Ordering;
    let slot = armed_slot();
    slot.store(0, Ordering::Relaxed);
    let now = timer.now();
    let base = now.saturating_add(100_000_000);
    arm_wakeup(timer, base);
    let after_base = slot.load(Ordering::Relaxed);
    arm_wakeup(timer, base.saturating_add(50_000_000));
    let after_later = slot.load(Ordering::Relaxed);
    let earlier = now.saturating_add(10_000_000);
    arm_wakeup(timer, earlier);
    let after_earlier = slot.load(Ordering::Relaxed);
    slot.store(0, Ordering::Relaxed);
    after_base == base && after_later == base && after_earlier == earlier
}

/// Earliest STILL-PENDING deadline among blocked tasks (IPC recv/send,
/// waitset, fence) — the sources that arm via `arm_wakeup` and that the
/// timer-IRQ re-arm previously dropped (it considered only timer caps).
/// Already-elapsed deadlines are skipped: their tasks are woken by the expiry
/// walks in this same handler pass, and arming a past deadline would fire a
/// spurious immediate IRQ.
fn earliest_blocked_deadline(tasks: &task::TaskTable, now: u64) -> Option<u64> {
    let mut earliest: Option<u64> = None;
    for pid_usize in 0..tasks.len() {
        let pid = task::Pid::from_raw(pid_usize as u32);
        let Some(t) = tasks.task(pid) else {
            continue;
        };
        if !t.is_blocked() {
            continue;
        }
        let d = match t.block_reason() {
            Some(task::BlockReason::IpcRecv { deadline_ns, .. })
            | Some(task::BlockReason::IpcSend { deadline_ns, .. })
            | Some(task::BlockReason::Waitset { deadline_ns, .. })
            | Some(task::BlockReason::Fence { deadline_ns, .. }) => deadline_ns,
            _ => 0,
        };
        if d > now {
            earliest = Some(earliest.map_or(d, |e| e.min(d)));
        }
    }
    earliest
}

pub(crate) fn process_expired_timers(
    timer: &dyn Timer,
    hart_timers: &mut crate::timer::HartTimers,
    router: &mut ipc::Router,
    tasks: &mut task::TaskTable,
    scheduler: &mut Scheduler,
) {
    let now = timer.now();
    for (timer_id, state) in hart_timers.pop_expired(now) {
        let mut payload = [0u8; 29];
        payload[0] = OP_TIMER_FIRED;
        payload[1..5].copy_from_slice(&timer_id.0.to_le_bytes());
        payload[5..9].copy_from_slice(&(state.seq.wrapping_add(1)).to_le_bytes());
        payload[9..13].copy_from_slice(&state.missed.to_le_bytes());
        payload[13..21].copy_from_slice(&state.deadline_ns.to_le_bytes());
        payload[21..29].copy_from_slice(&now.to_le_bytes());
        let header = ipc::header::MessageHeader::new(
            0,
            state.notify_ep,
            OP_TIMER_FIRED as u16,
            0,
            payload.len() as u32,
        );
        let msg = ipc::Message::new(header, alloc::vec::Vec::from(payload), None);
        if router.send(state.notify_ep, msg).is_ok() {
            if let Ok(Some(waiter)) = router.pop_recv_waiter(state.notify_ep) {
                let _ = tasks.wake(task::Pid::from_raw(waiter), scheduler);
            }
        }
    }

    // ADR-0052: this hart's armed deadline was consumed (timer IRQ) or is
    // being re-evaluated (idle loop) — clear the shadow, then re-arm to the
    // TRUE earliest across every deadline source: timer caps AND blocked-task
    // IPC/waitset/fence deadlines. Previously only timer caps were considered,
    // so this re-arm clobbered any armed pacer/IPC deadline to the 10ms
    // fallback tick. The fallback heartbeat stays the safety net.
    armed_slot().store(0, core::sync::atomic::Ordering::Relaxed);
    let mut next = hart_timers.earliest_deadline();
    if let Some(d) = earliest_blocked_deadline(tasks, now) {
        next = Some(next.map_or(d, |n| n.min(d)));
    }
    let fallback_ns = now.saturating_add(DEFAULT_TICK_CYCLES.saturating_mul(100));
    arm_wakeup(timer, next.unwrap_or(fallback_ns));
}

/// Wake every task whose IPC recv/send deadline has elapsed. A timed recv/send
/// (`Wait::Timeout`) arms the supervisor timer to its deadline via `set_wakeup`,
/// so the timer IRQ honors it here — making timed IPC waits reactive without
/// busy-polling (windowd's 120Hz pacer, gpud's spin-blur re-present). Without
/// this the timer handler only delivered timer caps + device IRQs, so a peer
/// blocked purely on an IPC deadline was never woken by the IRQ. Caller must hold
/// no aliasing borrows (the timer handler calls this only on a U-mode interrupt).
pub(crate) fn wake_expired_ipc_deadlines(
    timer: &dyn Timer,
    router: &mut ipc::Router,
    tasks: &mut task::TaskTable,
    scheduler: &mut Scheduler,
) {
    let now = timer.now();
    let len = tasks.len();
    for pid_usize in 0..len {
        let pid = task::Pid::from_raw(pid_usize as u32);
        let Some(t) = tasks.task(pid) else {
            continue;
        };
        if !t.is_blocked() {
            continue;
        }
        match t.block_reason() {
            Some(task::BlockReason::IpcRecv { endpoint, deadline_ns })
                if deadline_ns != 0 && now >= deadline_ns =>
            {
                let _ = router.remove_recv_waiter(endpoint, pid.as_raw());
                let _ = tasks.wake(pid, scheduler);
            }
            Some(task::BlockReason::IpcSend { endpoint, deadline_ns })
                if deadline_ns != 0 && now >= deadline_ns =>
            {
                let _ = router.remove_send_waiter(endpoint, pid.as_raw());
                let _ = tasks.wake(pid, scheduler);
            }
            _ => {}
        }
    }
}
