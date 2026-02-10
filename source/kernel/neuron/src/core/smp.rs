// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: TASK-0012 SMP v1 scaffolding (CPU identity, online mask, secondary boot, IPI bookkeeping)
//! OWNERS: @kernel-team
//! STATUS: In Progress
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU SMP marker path + kernel selftests
//! PUBLIC API: cpu_current_id(), cpu_online_mask(), start_secondary_harts(), request_resched()
//! DEPENDS_ON: arch::riscv::read_mhartid, sbi-rt (HSM/SPI), trap stack table consumed by trap.S
//! INVARIANTS: bounded CPU set, atomic online-mask updates, deterministic markers
//! ADR: docs/rfcs/RFC-0021-kernel-smp-v1-percpu-runqueues-ipi-contract.md

use core::sync::atomic::{AtomicUsize, Ordering};

use crate::types::{CpuId, HartId};

#[cfg(all(target_arch = "riscv64", target_os = "none"))]
use sbi_rt as sbi;

/// Fixed v1 CPU ceiling for deterministic bring-up and bounded per-CPU state.
pub const MAX_CPUS: usize = 4;

const SECONDARY_STACK_SIZE: usize = 16 * 1024;
const SBI_ERR_INVALID_PARAM: usize = (-3isize) as usize;
const SBI_ERR_ALREADY_AVAILABLE: usize = (-6isize) as usize;
const SBI_ERR_ALREADY_STARTED: usize = (-7isize) as usize;

#[derive(Clone, Copy)]
#[repr(align(16))]
#[allow(dead_code)]
struct HartStack([u8; SECONDARY_STACK_SIZE]);

const EMPTY_HART_STACK: HartStack = HartStack([0; SECONDARY_STACK_SIZE]);

// Dedicated secondary-hart stacks used as SBI HSM `hart_start` opaque stack tops.
#[link_section = ".bss"]
static mut SECONDARY_HART_STACKS: [HartStack; MAX_CPUS - 1] = [EMPTY_HART_STACK; MAX_CPUS - 1];

/// Trap stack table consumed from `arch/riscv/trap.S` on U-mode trap entry.
#[no_mangle]
pub static mut __hart_trap_stack_tops: [usize; MAX_CPUS] = [0; MAX_CPUS];

static CPU_ONLINE_MASK: AtomicUsize = AtomicUsize::new(0);
static RESCHED_PENDING: [AtomicUsize; MAX_CPUS] = [const { AtomicUsize::new(0) }; MAX_CPUS];
static RESCHED_REQ_ACCEPTED: [AtomicUsize; MAX_CPUS] = [const { AtomicUsize::new(0) }; MAX_CPUS];
static RESCHED_IPI_SENT_OK: [AtomicUsize; MAX_CPUS] = [const { AtomicUsize::new(0) }; MAX_CPUS];
static RESCHED_SSOFT_TRAPS: [AtomicUsize; MAX_CPUS] = [const { AtomicUsize::new(0) }; MAX_CPUS];
static RESCHED_ACK: [AtomicUsize; MAX_CPUS] = [const { AtomicUsize::new(0) }; MAX_CPUS];
static WORK_STEAL_EVENTS: AtomicUsize = AtomicUsize::new(0);
static SELFTEST_FORCE_IPI_SEND_FAIL: AtomicUsize = AtomicUsize::new(0);

#[cfg(all(target_arch = "riscv64", target_os = "none"))]
core::arch::global_asm!(
    r#"
    .section .text.__secondary_hart_start, "ax", @progbits
    .globl __secondary_hart_start
    .type  __secondary_hart_start, @function
    .align 4
__secondary_hart_start:
    /* SBI HSM contract: a0=hartid, a1=opaque. We pass stack-top via opaque. */
    mv    sp, a1
    .option push
    .option norelax
    la    gp, __global_pointer$
    .option pop
    tail  __secondary_hart_rust
    .size __secondary_hart_start, .-__secondary_hart_start
"#
);

#[cfg(all(target_arch = "riscv64", target_os = "none"))]
extern "C" {
    fn __secondary_hart_start();
}

#[no_mangle]
extern "C" fn __secondary_hart_rust(hartid: usize, stack_top: usize) -> ! {
    crate::kmain::kmain_secondary(HartId::from_raw(hartid as u16), stack_top)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReschedEvidence {
    pub request_accepted_count: usize,
    pub ipi_send_ok_count: usize,
    pub ssoft_trap_count: usize,
    pub ack_count: usize,
}

#[cfg(all(target_arch = "riscv64", target_os = "none"))]
fn cpu_from_stack_pointer(sp: usize) -> Option<CpuId> {
    for idx in 1..MAX_CPUS {
        let cpu = CpuId::from_raw(idx as u16);
        let Some(top) = secondary_stack_top(cpu) else {
            continue;
        };
        let base = top.saturating_sub(SECONDARY_STACK_SIZE);
        if sp >= base && sp <= top {
            return Some(cpu);
        }
    }
    None
}

#[inline]
pub fn cpu_current_id() -> CpuId {
    // S-mode must not rely on mhartid CSR reads (illegal on typical firmware).
    // We derive secondary-hart identity from the active stack range.
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        let sp = crate::arch::riscv::read_sp();
        if let Some(cpu) = cpu_from_stack_pointer(sp) {
            return cpu;
        }
    }
    CpuId::BOOT
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
    let idx = cpu.as_index();
    if idx >= MAX_CPUS {
        return;
    }
    // SAFETY: bounded by MAX_CPUS; called during deterministic bring-up paths.
    unsafe {
        __hart_trap_stack_tops[idx] = stack_top;
    }
}

pub fn trap_stack_top_for_current() -> usize {
    let idx = cpu_current_id().as_index();
    if idx < MAX_CPUS {
        // SAFETY: bounded index, table is initialized during hart bring-up.
        let top = unsafe { __hart_trap_stack_tops[idx] };
        if top != 0 {
            return top;
        }
    }
    linker_boot_stack_top()
}

/// Initializes boot CPU online/stack state for trap entry.
pub fn init_boot_hart_state() {
    let boot_cpu = CpuId::BOOT;
    register_trap_stack_top(boot_cpu, linker_boot_stack_top());
    mark_cpu_online(boot_cpu);
}

fn linker_boot_stack_top() -> usize {
    extern "C" {
        static __stack_top: u8;
    }
    // SAFETY: linker symbol points to static stack end in kernel image.
    unsafe { &__stack_top as *const u8 as usize }
}

fn secondary_stack_top(cpu: CpuId) -> Option<usize> {
    let idx = cpu.as_index();
    if idx == 0 || idx >= MAX_CPUS {
        return None;
    }
    // SAFETY: bounded index and static storage lifetime.
    let base = unsafe { core::ptr::addr_of!(SECONDARY_HART_STACKS[idx - 1]) as usize };
    Some(base + SECONDARY_STACK_SIZE)
}

/// Starts secondary harts via SBI HSM and returns the expected-online bitmask.
pub fn start_secondary_harts() -> usize {
    let boot = CpuId::BOOT;
    let mut expected_mask = 1usize << boot.as_index();

    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        for idx in 1..MAX_CPUS {
            let hart = HartId::from_raw(idx as u16);
            let cpu = CpuId::from_hart(hart);
            let Some(stack_top) = secondary_stack_top(cpu) else {
                continue;
            };

            let ret = sbi::hart_start(hart.as_index(), __secondary_hart_start as usize, stack_top);
            match ret.error {
                0 | SBI_ERR_ALREADY_AVAILABLE | SBI_ERR_ALREADY_STARTED => {
                    register_trap_stack_top(cpu, stack_top);
                    expected_mask |= 1usize << idx;
                }
                SBI_ERR_INVALID_PARAM => {
                    // No further harts are addressable in this environment.
                    break;
                }
                _ => {
                    log_error!(
                        target: "smp",
                        "KINIT: hart{} start failed err=0x{:x}",
                        idx,
                        ret.error
                    );
                    if idx == 1 {
                        panic!("SMP bring-up failed: hart1 not startable");
                    }
                }
            }
        }
    }

    expected_mask
}

pub fn wait_for_online_mask(expected_mask: usize, spin_budget: usize) -> bool {
    for _ in 0..spin_budget {
        if cpu_online_mask() & expected_mask == expected_mask {
            return true;
        }
        core::hint::spin_loop();
    }
    false
}

pub fn request_resched(target: CpuId) -> bool {
    let idx = target.as_index();
    if idx >= MAX_CPUS || !cpu_is_online(target) {
        return false;
    }
    RESCHED_REQ_ACCEPTED[idx].fetch_add(1, Ordering::AcqRel);
    RESCHED_PENDING[idx].store(1, Ordering::Release);

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
pub fn record_ssoft_trap(cpu: CpuId) {
    let idx = cpu.as_index();
    if idx >= MAX_CPUS {
        return;
    }
    RESCHED_SSOFT_TRAPS[idx].fetch_add(1, Ordering::AcqRel);
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
}
