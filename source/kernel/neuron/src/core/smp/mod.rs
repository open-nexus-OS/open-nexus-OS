// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the kernel's logical CPUs at run time (RFC-0021, sized by RFC-0118): CPU identity,
//! the online set, secondary boot, reschedule requests and their evidence. How many CPUs
//! there are and which hart each one is, is the machine's (`crate::topology`); the state of
//! one CPU lives in `hart_state`.
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU SMP marker path + kernel selftests
//! PUBLIC API: cpu_current_id(), cpu_online(), size_for_machine(), start_secondary_harts(), request_resched(), handle_ssoft_resched(), HartLocal prepare/adopt
//! DEPENDS_ON: sbi-rt (HSM/SPI), HartLocal blocks consumed by trap.S prologue (sscratch/tp ABI)
//! INVARIANTS: a CPU is an index, a hart id is what the firmware is told — never the other way
//!   round; atomic online-set updates; tp->HartLocal identity fast path with counted fallback
//! ADR: docs/rfcs/RFC-0021-kernel-smp-v1-percpu-runqueues-ipi-contract.md

use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use nexus_sched::CpuSet;

use crate::types::CpuId;

#[cfg(all(target_arch = "riscv64", target_os = "none"))]
use sbi_rt as sbi;

mod bringup;
pub(crate) mod hart_state;
mod lock_ping;
mod masks;
mod runtime;
pub mod tlb;

pub use masks::{cpu_idle, cpu_is_online, mark_cpu_idle};

pub use bringup::{
    emit_bringup_gate, note_bringup_stage, retry_missing_harts, stack_guards,
    start_secondary_harts, wait_for_online,
};
pub use lock_ping::{lock_ping_participate, selftest_lock_ping};
pub use runtime::{
    assign_spawn_cpu, mark_runtime_ready, record_timer_tick, record_user_dispatch, runtime_ready,
    set_wake_hint, steal_rate_gate, take_wake_hint, user_dispatch_total,
};

/// Per-hart kernel-local block. `sscratch` and (in S-mode) `tp` point at the
/// executing hart's instance; the trap prologue derives its stack and scratch
/// space from it, and `cpu_current_id()` derives CPU identity from `tp`.
///
/// Field order is an ABI with `arch/riscv/trap.S` (offsets injected via
/// `global_asm!` consts in `core/trap.rs`); asm-visible fields stay first.
#[repr(C)]
pub struct HartLocal {
    /// Trap stack top for this hart (trap.S: U-mode trap sp source).
    pub(crate) trap_stack_top: usize,
    /// trap.S prologue stash for the trapped `t1` (replaces the old stack red zone).
    pub(crate) scratch_t1: usize,
    /// trap.S prologue stash for the trapped `sp`.
    pub(crate) scratch_sp: usize,
    /// This hart's CPU index (identity fast path).
    pub(crate) cpu_index: usize,
    /// Validity tag so a bogus `tp` is never mistaken for a hart-local block.
    pub(crate) magic: usize,
    /// B (TASK-0042): per-hart preemption tick counter for the shares slice
    /// multiplier (only the owning hart touches it, from its timer trap).
    pub(crate) preempt_ticks: usize,
    /// Staging slot for the next context switch (A2b): the schedule decision
    /// copies the task frame here UNDER the BKL, the guard drops, then the
    /// sret path reads only this hart-local copy — no lock is ever held
    /// across a context switch.
    pub(crate) resume_frame: crate::trap::TrapFrame,
}

const HART_LOCAL_MAGIC: usize = 0x6e78_6861_7274_6c6f; // "nxhartlo"

impl HartLocal {
    const EMPTY: Self = Self {
        trap_stack_top: 0,
        scratch_t1: 0,
        scratch_sp: 0,
        cpu_index: 0,
        magic: 0,
        preempt_ticks: 0,
        resume_frame: crate::trap::TrapFrame::EMPTY,
    };
}

/// Cache-line-aligned wrapper: hart-locals must never share a line.
#[repr(C, align(64))]
struct HartLocalBlock(UnsafeCell<HartLocal>);

// SAFETY: Each block is written by its owning hart (or by the boot hart
// strictly before the owning hart starts, ordered by the SBI HSM hart_start
// call); the asm scratch fields are only touched by the owning hart's trap
// prologue with traps unable to nest.
unsafe impl Sync for HartLocalBlock {}

/// One block per CPU a [`CpuSet`] can name. The one per-hart table that is not sized by the
/// machine: the trap prologue finds a hart's block by its address (`sscratch`, `tp`), from the
/// first trap on — before any allocator runs — and `cpu_from_hart_local_tp` proves a `tp` by
/// this array's bounds. Blocks beyond the machine's harts stay zero pages of BSS.
static HART_LOCALS: [HartLocalBlock; CpuSet::BITS] =
    [const { HartLocalBlock(UnsafeCell::new(HartLocal::EMPTY)) }; CpuSet::BITS];

/// Counterfactual tripwire: how often CPU identity had to fall back to the
/// legacy heuristic because `tp` did not point at a valid hart-local block.
/// Selftests assert this stays 0 after bring-up.
static CPUID_FALLBACK_EVENTS: AtomicUsize = AtomicUsize::new(0);

/// The CPUs that are online (bit `n` = logical CPU `n`).
static CPU_ONLINE: AtomicU64 = AtomicU64::new(0);

/// `(sent, skipped)` wake interrupts across all CPUs — the skipped ones were elided because a
/// reschedule was already outstanding on the target; reported so the saving is visible.
#[must_use]
pub fn resched_ipi_counts() -> (usize, usize) {
    hart_state::all().fold((0, 0), |(sent, skipped), (_, hart)| {
        (
            sent + hart.resched_ipi_sent_ok.load(Ordering::Relaxed),
            skipped + hart.resched_ipi_skipped.load(Ordering::Relaxed),
        )
    })
}

/// Sizes every per-hart table of the kernel for the machine the tree describes and reserves
/// the secondaries' stacks — once, by the boot hart, after the heap and the frame pool exist
/// and before the kernel's address space is built (the stacks' guard pages are holes in it)
/// and before any secondary starts. A kernel that cannot hold its own harts' state does not
/// run: the failure is named on the console and the boot stops.
pub fn size_for_machine() {
    let harts = crate::topology::hart_count();
    let sized = hart_state::size(harts)
        .and_then(|()| bringup::size(harts))
        .and_then(|()| crate::hal::console_line::size(harts))
        .and_then(|()| crate::irq::size(harts))
        .and_then(|()| crate::trap::size_hart_deadlines(harts));
    if let Err(error) = sized {
        log_error!(target: "smp", "KPANIC: smp slab harts={} ({:?})", harts, error);
        panic!("smp slab: per-hart state not sized");
    }
    if let Err(error) = bringup::reserve_stacks(harts) {
        log_error!(target: "smp", "KPANIC: smp slab harts={} stacks ({:?})", harts, error);
        panic!("smp slab: secondary stacks not reserved");
    }
    crate::sync::hart_table::machine_sized(harts);
}

/// Raises the supervisor software interrupt on the CPUs of `targets`. The firmware is told
/// harts, the kernel speaks CPUs: the machine translates (`topology::plan_ipi`). `false` when
/// the firmware refused a call or the machine is not read yet.
#[cfg(all(target_arch = "riscv64", target_os = "none"))]
pub(crate) fn send_ipi(targets: CpuSet) -> bool {
    let Some(machine) = crate::topology::machine() else {
        return false;
    };
    let mut accepted = true;
    crate::topology::plan_ipi(machine, targets, |mask, base| {
        accepted &= sbi::send_ipi(mask, base).error == 0;
    });
    accepted
}

static WORK_STEAL_EVENTS: AtomicUsize = AtomicUsize::new(0);
static SELFTEST_FORCE_IPI_SEND_FAIL: AtomicUsize = AtomicUsize::new(0);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReschedEvidence {
    pub request_accepted_count: usize,
    pub ipi_send_ok_count: usize,
    pub ssoft_trap_count: usize,
    pub ack_count: usize,
}

#[must_use = "resched trap outcomes must be handled"]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReschedTrapOutcome {
    Acked,
    NoPendingRequest,
}

#[inline]
fn resolve_cpu_id(tp_hint: Option<CpuId>, stack_cpu: Option<CpuId>) -> CpuId {
    match (tp_hint, stack_cpu) {
        (Some(tp), Some(stack_cpu)) if tp == stack_cpu => tp,
        (_, Some(stack_cpu)) => stack_cpu,
        (Some(tp), None) if tp.is_boot() => tp,
        _ => CpuId::BOOT,
    }
}

fn hart_local_ptr(cpu: CpuId) -> *mut HartLocal {
    HART_LOCALS[cpu.as_index()].0.get()
}

/// Fills a hart's local block. Must happen strictly before that hart's trap
/// vector (or identity fast path) relies on it: the boot hart prepares
/// secondaries *before* `sbi::hart_start`, and each hart prepares itself
/// idempotently in its trap-install path.
pub fn hart_local_prepare(cpu: CpuId, trap_stack_top: usize) {
    let idx = cpu.as_index();
    if idx >= HART_LOCALS.len() {
        return;
    }
    // SAFETY: single-writer per block by the HSM ordering contract above; the
    // owning hart cannot concurrently trap on a block it has not adopted yet.
    unsafe {
        let block = hart_local_ptr(cpu);
        (*block).trap_stack_top = trap_stack_top;
        (*block).cpu_index = idx;
        (*block).magic = HART_LOCAL_MAGIC;
    }
}

/// Points this hart's `tp` at its local block (S-mode identity anchor).
/// The trap prologue re-establishes this after every trap entry.
#[cfg(all(target_arch = "riscv64", target_os = "none"))]
pub fn hart_local_adopt(cpu: CpuId) {
    let block = hart_local_ptr(cpu) as usize;
    // SAFETY: writing `tp` in S-mode kernel context; user `tp` is saved and
    // restored by the trap frame independently of this.
    unsafe {
        core::arch::asm!("mv tp, {b}", b = in(reg) block, options(nomem, nostack, preserves_flags));
    }
}

/// The `sscratch` value for a hart: the address of its local block.
pub fn hart_local_sscratch_value(cpu: CpuId) -> usize {
    hart_local_ptr(cpu) as usize
}

/// This hart's kernel stack top (also its trap stack top).
pub fn hart_stack_top(cpu: CpuId) -> usize {
    // SAFETY: bounds-checked read of a prepared block field.
    unsafe { (*hart_local_ptr(cpu)).trap_stack_top }
}

/// B (TASK-0042): shares slice gate. Counts this hart's preemption ticks and
/// returns whether the CURRENT task's slice is exhausted (rotate now).
/// `slice_ticks` >= 1; shares=100 → 1 tick (pre-B behavior, exact parity).
pub fn preempt_tick_and_rotate(cpu: CpuId, slice_ticks: usize) -> bool {
    // SAFETY: single-writer per block (owning hart's timer trap only).
    unsafe {
        let block = hart_local_ptr(cpu);
        let ticks = (*block).preempt_ticks.wrapping_add(1);
        (*block).preempt_ticks = ticks;
        slice_ticks <= 1 || ticks % slice_ticks == 0
    }
}

/// Stages a task frame into this hart's resume slot (A2b contract: written
/// under the BKL, consumed lock-free by the sret path). The pointer stays
/// valid until the same hart stages again.
pub fn hart_local_stage_resume(
    cpu: CpuId,
    frame: &crate::trap::TrapFrame,
) -> *const crate::trap::TrapFrame {
    // SAFETY: only the owning hart stages and consumes its resume slot, and
    // it does both within one idle-loop iteration (no concurrent access).
    unsafe {
        let block = hart_local_ptr(cpu);
        (*block).resume_frame = *frame;
        core::ptr::addr_of!((*block).resume_frame)
    }
}

/// Identity fast path: `tp` points at a valid hart-local block in S-mode. `None` before the
/// block is installed — the console's line discipline (`hal::console_line`) asks here on every
/// byte and must never count as an identity fallback.
#[cfg(all(target_arch = "riscv64", target_os = "none"))]
pub(crate) fn cpu_from_hart_local_tp() -> Option<CpuId> {
    let raw_tp: usize;
    // SAFETY: reading `tp` is side-effect free.
    unsafe {
        core::arch::asm!("mv {o}, tp", o = out(reg) raw_tp, options(nomem, nostack, preserves_flags));
    }
    let base = HART_LOCALS.as_ptr() as usize;
    let stride = core::mem::size_of::<HartLocalBlock>();
    let end = base + stride * HART_LOCALS.len();
    if raw_tp < base || raw_tp >= end || (raw_tp - base) % stride != 0 {
        return None;
    }
    let idx = (raw_tp - base) / stride;
    // SAFETY: bounds-checked pointer into HART_LOCALS; reads are plain loads.
    let (magic, cpu_index) = unsafe {
        let block = raw_tp as *const HartLocal;
        ((*block).magic, (*block).cpu_index)
    };
    if magic != HART_LOCAL_MAGIC || cpu_index != idx {
        return None;
    }
    Some(CpuId::from_raw(idx as u16))
}

/// Counterfactual counter: identity resolutions that missed the tp fast path.
#[inline]
pub fn cpuid_fallback_count() -> usize {
    CPUID_FALLBACK_EVENTS.load(Ordering::Acquire)
}

#[inline]
pub fn cpu_current_id() -> CpuId {
    // S-mode must not rely on mhartid CSR reads (illegal on typical firmware).
    // Fast path: `tp` points at this hart's local block (installed at hart
    // entry, re-established by the trap prologue).
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        if let Some(cpu) = cpu_from_hart_local_tp() {
            return cpu;
        }
        // Legacy heuristic fallback, kept for exactly one proven-green cycle;
        // the counter is asserted 0 by KSELFTEST (fake-proof tripwire).
        CPUID_FALLBACK_EVENTS.fetch_add(1, Ordering::AcqRel);
        let sp = crate::arch::riscv::read_sp();
        let stack_cpu = bringup::cpu_of_stack(sp);
        resolve_cpu_id(None, stack_cpu)
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        CpuId::BOOT
    }
}

/// Selftest probe: verifies that a poisoned `tp` is (a) rejected by the fast
/// path and (b) counted as a fallback event, then restores identity.
/// Returns `(resolved_cpu, fallback_delta)` for marker evaluation.
#[cfg(all(target_arch = "riscv64", target_os = "none"))]
pub fn selftest_poisoned_tp_probe() -> (CpuId, usize) {
    let _irq = crate::sync::spin_irq::IrqOffGuard::new();
    let before = cpuid_fallback_count();
    let saved: usize;
    // SAFETY: tp is saved and restored within an IRQ-off window on this hart;
    // no trap can observe the poisoned value.
    unsafe {
        core::arch::asm!("mv {o}, tp", o = out(reg) saved, options(nomem, nostack, preserves_flags));
        core::arch::asm!("mv tp, {b}", b = in(reg) usize::MAX, options(nomem, nostack, preserves_flags));
    }
    let resolved = cpu_current_id();
    // SAFETY: restores the exact saved tp.
    unsafe {
        core::arch::asm!("mv tp, {b}", b = in(reg) saved, options(nomem, nostack, preserves_flags));
    }
    (resolved, cpuid_fallback_count() - before)
}

/// The CPUs that are online.
#[inline]
pub fn cpu_online() -> CpuSet {
    CpuSet::from_bits(CPU_ONLINE.load(Ordering::Acquire))
}

/// The online CPUs other than the boot hart's.
pub fn secondaries_online() -> impl Iterator<Item = CpuId> {
    cpu_online().iter().skip_while(|&cpu| cpu == 0).map(|cpu| CpuId::from_raw(cpu as u16))
}

/// Emits deterministic online markers exactly once per CPU. A CPU the machine does not have
/// never comes online.
pub fn mark_cpu_online(cpu: CpuId) {
    let idx = cpu.as_index();
    let Some(one) = CpuSet::single(idx).filter(|_| idx < crate::topology::hart_count()) else {
        return;
    };
    let previous = CpuSet::from_bits(CPU_ONLINE.fetch_or(one.bits(), Ordering::AcqRel));
    if !previous.contains(idx) {
        log_info!(target: "smp", "KINIT: cpu{} online", idx);
    }
}

pub fn register_trap_stack_top(cpu: CpuId, stack_top: usize) {
    hart_local_prepare(cpu, stack_top);
}

/// Initializes boot CPU online/stack state for trap entry.
pub fn init_boot_hart_state() {
    let boot_cpu = CpuId::BOOT;
    register_trap_stack_top(boot_cpu, linker_boot_stack_top());
    mark_cpu_online(boot_cpu);
}

pub(crate) fn linker_boot_stack_top() -> usize {
    extern "C" {
        static __stack_top: u8;
    }
    // SAFETY: linker symbol points to static stack end in kernel image.
    unsafe { &__stack_top as *const u8 as usize }
}

pub fn request_resched(target: CpuId) -> bool {
    let idx = target.as_index();
    let Some(hart) = hart_state::of(target).filter(|_| cpu_is_online(target)) else {
        return false;
    };
    hart.resched_req_accepted.fetch_add(1, Ordering::AcqRel);
    // CLAIM, not a store: `take_resched` consumes the flag with `swap(0)`, so
    // a non-zero previous value means a resched is ALREADY outstanding on that
    // CPU and a second IPI buys nothing — whoever consumes the flag rescans
    // the whole run queue and will find this task too.
    //
    // Safe because the enqueue happens-before this Release swap, and the
    // consumer's Acquire swap synchronizes with the latest Release in the
    // modification order. A consumer that observes ANY pending flag therefore
    // observes this enqueue. If we are the one who set it (prev == 0), we send.
    //
    // Worth doing because the IPI is not cheap: measured at ~50-60 us mean and
    // up to 6.3 ms, and it runs inside the held BKL (TASK-0306 Phase 3).
    let already_pending = hart.resched_pending.swap(1, Ordering::AcqRel) != 0;
    hart.wake_hint.store(1, Ordering::Release);

    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        if already_pending {
            hart.resched_ipi_skipped.fetch_add(1, Ordering::Relaxed);
        } else if SELFTEST_FORCE_IPI_SEND_FAIL.load(Ordering::Acquire) == 0
            && CpuSet::single(idx).is_some_and(send_ipi)
        {
            hart.resched_ipi_sent_ok.fetch_add(1, Ordering::AcqRel);
        }
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    let _ = (already_pending, idx);

    true
}

#[inline]
pub fn take_resched(cpu: CpuId) -> bool {
    hart_state::of(cpu).is_some_and(|hart| hart.resched_pending.swap(0, Ordering::AcqRel) != 0)
}

#[inline]
pub fn acknowledge_resched(cpu: CpuId) {
    if let Some(hart) = hart_state::of(cpu) {
        hart.resched_ack.fetch_add(1, Ordering::AcqRel);
    }
}

#[inline]
pub fn handle_ssoft_resched(cpu: CpuId) -> ReschedTrapOutcome {
    record_ssoft_trap(cpu);
    if take_resched(cpu) {
        acknowledge_resched(cpu);
        ReschedTrapOutcome::Acked
    } else {
        ReschedTrapOutcome::NoPendingRequest
    }
}

#[inline]
pub fn record_ssoft_trap(cpu: CpuId) {
    let Some(hart) = hart_state::of(cpu) else {
        return;
    };
    hart.resched_ssoft_traps.fetch_add(1, Ordering::AcqRel);
    // Bounded bring-up diagnostic: first few S_SOFT traps per boot.
    static SSOFT_LOGGED: AtomicUsize = AtomicUsize::new(0);
    if SSOFT_LOGGED.fetch_add(1, Ordering::Relaxed) < 4 {
        log_info!(target: "smp", "KINIT: cpu{} ssoft trap", cpu.as_index());
    }
}

#[inline]
pub fn clear_resched_pending(cpu: CpuId) {
    if let Some(hart) = hart_state::of(cpu) {
        hart.resched_pending.store(0, Ordering::Release);
    }
}

#[inline]
pub fn resched_evidence(cpu: CpuId) -> ReschedEvidence {
    let Some(hart) = hart_state::of(cpu) else {
        return ReschedEvidence::default();
    };
    ReschedEvidence {
        request_accepted_count: hart.resched_req_accepted.load(Ordering::Acquire),
        ipi_send_ok_count: hart.resched_ipi_sent_ok.load(Ordering::Acquire),
        ssoft_trap_count: hart.resched_ssoft_traps.load(Ordering::Acquire),
        ack_count: hart.resched_ack.load(Ordering::Acquire),
    }
}

pub fn selftest_force_ipi_send_failure(enable: bool) {
    SELFTEST_FORCE_IPI_SEND_FAIL.store(enable as usize, Ordering::Release);
}

#[inline]
pub fn record_work_steal() {
    WORK_STEAL_EVENTS.fetch_add(1, Ordering::AcqRel);
}

#[inline]
pub fn work_steal_count() -> usize {
    WORK_STEAL_EVENTS.load(Ordering::Acquire)
}

pub fn reset_selftest_counters() {
    for (_, hart) in hart_state::all() {
        hart.resched_pending.store(0, Ordering::Release);
        hart.resched_req_accepted.store(0, Ordering::Release);
        hart.resched_ipi_sent_ok.store(0, Ordering::Release);
        hart.resched_ssoft_traps.store(0, Ordering::Release);
        hart.resched_ack.store(0, Ordering::Release);
    }
    SELFTEST_FORCE_IPI_SEND_FAIL.store(0, Ordering::Release);
    WORK_STEAL_EVENTS.store(0, Ordering::Release);
}

#[cfg(test)]
mod tests {
    use super::*;
    use spin::Mutex;

    static TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn test_reject_invalid_ipi_target_cpu() {
        let _guard = TEST_LOCK.lock();
        reset_selftest_counters();
        CPU_ONLINE.store(CpuSet::first(1).bits(), Ordering::Release);

        let invalid = CpuId::from_raw(CpuSet::BITS as u16);
        assert!(!request_resched(invalid));
        assert_eq!(resched_evidence(CpuId::BOOT).request_accepted_count, 0);
    }

    #[test]
    fn test_reject_offline_cpu_resched() {
        let _guard = TEST_LOCK.lock();
        reset_selftest_counters();
        CPU_ONLINE.store(CpuSet::first(1).bits(), Ordering::Release);

        let offline = CpuId::from_raw(1);
        assert!(!request_resched(offline));
        assert_eq!(resched_evidence(offline).request_accepted_count, 0);
    }

    #[test]
    fn test_ssoft_contract_acknowledges_pending_request() {
        let _guard = TEST_LOCK.lock();
        reset_selftest_counters();
        let target = CpuId::from_raw(1);
        CPU_ONLINE.store(CpuSet::first(2).bits(), Ordering::Release);

        assert!(request_resched(target));
        assert_eq!(handle_ssoft_resched(target), ReschedTrapOutcome::Acked);

        let evidence = resched_evidence(target);
        assert_eq!(evidence.request_accepted_count, 1);
        assert_eq!(evidence.ssoft_trap_count, 1);
        assert_eq!(evidence.ack_count, 1);
    }

    #[test]
    fn test_ssoft_contract_records_counterfactual_without_ack() {
        let _guard = TEST_LOCK.lock();
        reset_selftest_counters();
        let target = CpuId::from_raw(1);
        CPU_ONLINE.store(CpuSet::first(2).bits(), Ordering::Release);

        assert_eq!(handle_ssoft_resched(target), ReschedTrapOutcome::NoPendingRequest);

        let evidence = resched_evidence(target);
        assert_eq!(evidence.request_accepted_count, 0);
        assert_eq!(evidence.ssoft_trap_count, 1);
        assert_eq!(evidence.ack_count, 0);
    }

    #[test]
    fn test_cpu_id_resolution_prefers_stack_on_tp_mismatch() {
        let tp_hint = Some(CpuId::BOOT);
        let stack_cpu = Some(CpuId::from_raw(1));
        assert_eq!(resolve_cpu_id(tp_hint, stack_cpu), CpuId::from_raw(1));
    }

    #[test]
    fn test_cpu_id_resolution_uses_boot_when_only_tp_non_boot_exists() {
        let tp_hint = Some(CpuId::from_raw(1));
        let stack_cpu = None;
        assert_eq!(resolve_cpu_id(tp_hint, stack_cpu), CpuId::BOOT);
    }
}
