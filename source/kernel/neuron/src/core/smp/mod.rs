// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: TASK-0012 SMP v1 scaffolding (CPU identity, online mask, secondary boot, IPI bookkeeping)
//! OWNERS: @kernel-team
//! STATUS: In Progress
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU SMP marker path + kernel selftests
//! PUBLIC API: cpu_current_id(), cpu_online_mask(), start_secondary_harts(), request_resched(), handle_ssoft_resched(), HartLocal prepare/adopt
//! DEPENDS_ON: sbi-rt (HSM/SPI), HartLocal blocks consumed by trap.S prologue (sscratch/tp ABI)
//! INVARIANTS: bounded CPU set, atomic online-mask updates, tp->HartLocal identity fast path with counted fallback, deterministic markers
//! ADR: docs/rfcs/RFC-0021-kernel-smp-v1-percpu-runqueues-ipi-contract.md

use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicUsize, Ordering};

use crate::types::CpuId;

#[cfg(all(target_arch = "riscv64", target_os = "none"))]
use sbi_rt as sbi;

/// Fixed v1 CPU ceiling for deterministic bring-up and bounded per-CPU state.
pub const MAX_CPUS: usize = 4;

mod bringup;
mod runtime;

pub use bringup::{
    emit_bringup_gate, retry_missing_harts, start_secondary_harts, wait_for_online_mask,
    BRINGUP_STAGE,
};
pub use runtime::{
    assign_spawn_cpu, mark_runtime_ready, record_timer_tick, record_user_dispatch,
    request_lazy_tlb_flush_others, runtime_ready, steal_rate_gate, take_lazy_tlb_flush,
    take_wake_hint,
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

static HART_LOCALS: [HartLocalBlock; MAX_CPUS] =
    [const { HartLocalBlock(UnsafeCell::new(HartLocal::EMPTY)) }; MAX_CPUS];

/// Counterfactual tripwire: how often CPU identity had to fall back to the
/// legacy heuristic because `tp` did not point at a valid hart-local block.
/// Selftests assert this stays 0 after bring-up.
static CPUID_FALLBACK_EVENTS: AtomicUsize = AtomicUsize::new(0);

static CPU_ONLINE_MASK: AtomicUsize = AtomicUsize::new(0);
static RESCHED_PENDING: [AtomicUsize; MAX_CPUS] = [const { AtomicUsize::new(0) }; MAX_CPUS];
static RESCHED_REQ_ACCEPTED: [AtomicUsize; MAX_CPUS] = [const { AtomicUsize::new(0) }; MAX_CPUS];
static RESCHED_IPI_SENT_OK: [AtomicUsize; MAX_CPUS] = [const { AtomicUsize::new(0) }; MAX_CPUS];
static RESCHED_SSOFT_TRAPS: [AtomicUsize; MAX_CPUS] = [const { AtomicUsize::new(0) }; MAX_CPUS];
static RESCHED_ACK: [AtomicUsize; MAX_CPUS] = [const { AtomicUsize::new(0) }; MAX_CPUS];
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

#[cfg(test)]
#[inline]
fn cpu_from_tp_hint_raw(raw_tp: usize, online_mask: usize) -> Option<CpuId> {
    if raw_tp >= MAX_CPUS {
        return None;
    }
    let cpu = CpuId::from_raw(raw_tp as u16);
    let bit = 1usize << cpu.as_index();
    if online_mask == 0 || (online_mask & bit) != 0 {
        Some(cpu)
    } else {
        None
    }
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

#[cfg(all(target_arch = "riscv64", target_os = "none"))]
fn cpu_from_stack_pointer(sp: usize) -> Option<CpuId> {
    for idx in 1..MAX_CPUS {
        let cpu = CpuId::from_raw(idx as u16);
        let Some(top) = bringup::secondary_stack_top(cpu) else {
            continue;
        };
        let base = top.saturating_sub(bringup::SECONDARY_STACK_SIZE);
        if sp >= base && sp <= top {
            return Some(cpu);
        }
    }
    None
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
    if idx >= MAX_CPUS {
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

/// Identity fast path: `tp` points at a valid hart-local block in S-mode.
#[cfg(all(target_arch = "riscv64", target_os = "none"))]
fn cpu_from_hart_local_tp() -> Option<CpuId> {
    let raw_tp: usize;
    // SAFETY: reading `tp` is side-effect free.
    unsafe {
        core::arch::asm!("mv {o}, tp", o = out(reg) raw_tp, options(nomem, nostack, preserves_flags));
    }
    let base = HART_LOCALS.as_ptr() as usize;
    let stride = core::mem::size_of::<HartLocalBlock>();
    let end = base + stride * MAX_CPUS;
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
        let stack_cpu = cpu_from_stack_pointer(sp);
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

#[inline]
pub fn cpu_online_mask() -> usize {
    CPU_ONLINE_MASK.load(Ordering::Acquire)
}

#[inline]
pub fn cpu_is_online(cpu: CpuId) -> bool {
    let bit = 1usize << cpu.as_index();
    cpu_online_mask() & bit != 0
}

/// Emits deterministic online markers exactly once per CPU.
pub fn mark_cpu_online(cpu: CpuId) {
    let idx = cpu.as_index();
    if idx >= MAX_CPUS {
        return;
    }
    let bit = 1usize << idx;
    let previous = CPU_ONLINE_MASK.fetch_or(bit, Ordering::AcqRel);
    if previous & bit == 0 {
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
    if idx >= MAX_CPUS || !cpu_is_online(target) {
        return false;
    }
    RESCHED_REQ_ACCEPTED[idx].fetch_add(1, Ordering::AcqRel);
    RESCHED_PENDING[idx].store(1, Ordering::Release);
    runtime::set_wake_hint(idx);

    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        if idx < usize::BITS as usize {
            if SELFTEST_FORCE_IPI_SEND_FAIL.load(Ordering::Acquire) == 0 {
                let ret = sbi::send_ipi(1usize << idx, 0);
                if ret.error == 0 {
                    RESCHED_IPI_SENT_OK[idx].fetch_add(1, Ordering::AcqRel);
                }
            }
        }
    }

    true
}

#[inline]
pub fn take_resched(cpu: CpuId) -> bool {
    let idx = cpu.as_index();
    if idx >= MAX_CPUS {
        return false;
    }
    RESCHED_PENDING[idx].swap(0, Ordering::AcqRel) != 0
}

#[inline]
pub fn acknowledge_resched(cpu: CpuId) {
    let idx = cpu.as_index();
    if idx >= MAX_CPUS {
        return;
    }
    RESCHED_ACK[idx].fetch_add(1, Ordering::AcqRel);
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
    let idx = cpu.as_index();
    if idx >= MAX_CPUS {
        return;
    }
    RESCHED_SSOFT_TRAPS[idx].fetch_add(1, Ordering::AcqRel);
    // Bounded bring-up diagnostic: first few S_SOFT traps per boot.
    static SSOFT_LOGGED: AtomicUsize = AtomicUsize::new(0);
    if SSOFT_LOGGED.fetch_add(1, Ordering::Relaxed) < 4 {
        log_info!(target: "smp", "KINIT: cpu{} ssoft trap", idx);
    }
}

#[inline]
pub fn clear_resched_pending(cpu: CpuId) {
    let idx = cpu.as_index();
    if idx >= MAX_CPUS {
        return;
    }
    RESCHED_PENDING[idx].store(0, Ordering::Release);
}

#[inline]
pub fn resched_evidence(cpu: CpuId) -> ReschedEvidence {
    let idx = cpu.as_index();
    if idx >= MAX_CPUS {
        return ReschedEvidence::default();
    }
    ReschedEvidence {
        request_accepted_count: RESCHED_REQ_ACCEPTED[idx].load(Ordering::Acquire),
        ipi_send_ok_count: RESCHED_IPI_SENT_OK[idx].load(Ordering::Acquire),
        ssoft_trap_count: RESCHED_SSOFT_TRAPS[idx].load(Ordering::Acquire),
        ack_count: RESCHED_ACK[idx].load(Ordering::Acquire),
    }
}

// ——— A2 lock-ping selftest: proves SpinIrqLock excludes across real harts ———

static LOCK_PING_COUNTER: crate::sync::spin_irq::SpinIrqLock<usize> =
    crate::sync::spin_irq::SpinIrqLock::new(0);
static LOCK_PING_ROUNDS: AtomicUsize = AtomicUsize::new(0);
static LOCK_PING_ACKS: AtomicUsize = AtomicUsize::new(0);

/// Secondary-hart side: performs the requested lock-ping rounds exactly once.
/// Called from the secondary park loop.
pub fn lock_ping_participate(participated: &mut bool) {
    if *participated {
        return;
    }
    let rounds = LOCK_PING_ROUNDS.load(Ordering::Acquire);
    if rounds == 0 {
        return;
    }
    for _ in 0..rounds {
        let mut counter = LOCK_PING_COUNTER.lock();
        *counter += 1;
    }
    LOCK_PING_ACKS.fetch_add(1, Ordering::AcqRel);
    *participated = true;
}

/// Boot-hart side: runs a bounded two-(or more-)hart lock ping and returns
/// `(final_counter, acked_secondaries)`. Deterministic result proof: with
/// `n` acked participants the counter must be exactly `rounds * (1 + n)` —
/// a broken lock loses increments, a fake ack inflates none.
pub fn selftest_lock_ping(rounds: usize, spin_budget: usize) -> (usize, usize) {
    {
        let mut counter = LOCK_PING_COUNTER.lock();
        *counter = 0;
    }
    LOCK_PING_ACKS.store(0, Ordering::Release);
    LOCK_PING_ROUNDS.store(rounds, Ordering::Release);
    // Parked secondaries WFI; punch them out so they observe the request.
    for idx in 1..MAX_CPUS {
        let target = CpuId::from_raw(idx as u16);
        if cpu_is_online(target) {
            let _ = request_resched(target);
        }
    }

    for _ in 0..rounds {
        let mut counter = LOCK_PING_COUNTER.lock();
        *counter += 1;
    }

    let expected_acks = cpu_online_mask().count_ones().saturating_sub(1) as usize;
    for _ in 0..spin_budget {
        if LOCK_PING_ACKS.load(Ordering::Acquire) >= expected_acks {
            break;
        }
        core::hint::spin_loop();
    }
    LOCK_PING_ROUNDS.store(0, Ordering::Release);

    let total = *LOCK_PING_COUNTER.lock();
    (total, LOCK_PING_ACKS.load(Ordering::Acquire))
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
    for counter in &RESCHED_PENDING {
        counter.store(0, Ordering::Release);
    }
    for counter in &RESCHED_REQ_ACCEPTED {
        counter.store(0, Ordering::Release);
    }
    for counter in &RESCHED_IPI_SENT_OK {
        counter.store(0, Ordering::Release);
    }
    for counter in &RESCHED_SSOFT_TRAPS {
        counter.store(0, Ordering::Release);
    }
    for counter in &RESCHED_ACK {
        counter.store(0, Ordering::Release);
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
        CPU_ONLINE_MASK.store(1usize << CpuId::BOOT.as_index(), Ordering::Release);

        let invalid = CpuId::from_raw(MAX_CPUS as u16);
        assert!(!request_resched(invalid));
        assert_eq!(resched_evidence(CpuId::BOOT).request_accepted_count, 0);
    }

    #[test]
    fn test_reject_offline_cpu_resched() {
        let _guard = TEST_LOCK.lock();
        reset_selftest_counters();
        CPU_ONLINE_MASK.store(1usize << CpuId::BOOT.as_index(), Ordering::Release);

        let offline = CpuId::from_raw(1);
        assert!(!request_resched(offline));
        assert_eq!(resched_evidence(offline).request_accepted_count, 0);
    }

    #[test]
    fn test_ssoft_contract_acknowledges_pending_request() {
        let _guard = TEST_LOCK.lock();
        reset_selftest_counters();
        let target = CpuId::from_raw(1);
        CPU_ONLINE_MASK.store(
            (1usize << CpuId::BOOT.as_index()) | (1usize << target.as_index()),
            Ordering::Release,
        );

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
        CPU_ONLINE_MASK.store(
            (1usize << CpuId::BOOT.as_index()) | (1usize << target.as_index()),
            Ordering::Release,
        );

        assert_eq!(handle_ssoft_resched(target), ReschedTrapOutcome::NoPendingRequest);

        let evidence = resched_evidence(target);
        assert_eq!(evidence.request_accepted_count, 0);
        assert_eq!(evidence.ssoft_trap_count, 1);
        assert_eq!(evidence.ack_count, 0);
    }

    #[test]
    fn test_reject_tp_hint_for_offline_cpu() {
        let _guard = TEST_LOCK.lock();
        let online_mask = 1usize << CpuId::BOOT.as_index();
        assert_eq!(cpu_from_tp_hint_raw(1, online_mask), None);
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
