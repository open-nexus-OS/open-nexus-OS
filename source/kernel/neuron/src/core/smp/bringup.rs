// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Secondary-hart HSM bring-up — entry stub, sequential start with
//! settle-wait + bounded retry, and the self-diagnosing KGATE boot gate.
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU SMP proofs (KINIT: cpuN online, KGATE: smp ...)
//! INVARIANTS: a lost hart (firmware-side: HSM STARTED but never at our
//!   entry) degrades the boot loudly instead of killing it; every hart's
//!   bring-up stage is recorded for the gate.
//! ADR: docs/rfcs/RFC-0021-kernel-smp-v1-percpu-runqueues-ipi-contract.md

use core::sync::atomic::{AtomicUsize, Ordering};

use crate::types::{CpuId, HartId};

#[cfg(all(target_arch = "riscv64", target_os = "none"))]
use sbi_rt as sbi;

use super::{cpu_online_mask, hart_local_prepare, MAX_CPUS};

// Must match the boot-stack budget: secondaries run the FULL syscall path
// (incl. ELF-loading spawns) on this stack once they serve the runtime (A3).
// 32 KiB was measured too small for the deepest syscall (see kernel.ld note);
// an overflow here silently corrupts adjacent .bss.
pub(super) const SECONDARY_STACK_SIZE: usize = 64 * 1024;
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
    crate::cpu_main::kmain_secondary(HartId::from_raw(hartid as u16), stack_top)
}

/// Secondary bring-up stage per hart (diagnostic): 0=never entered Rust,
/// 1=entry, 2=hart-local prepared, 3=trap vector installed, 4=online.
/// Dumped by the boot hart when a hart goes missing during bring-up.
pub static BRINGUP_STAGE: [AtomicUsize; MAX_CPUS] = [const { AtomicUsize::new(0) }; MAX_CPUS];

pub(super) fn secondary_stack_top(cpu: CpuId) -> Option<usize> {
    let idx = cpu.as_index();
    if idx == 0 || idx >= MAX_CPUS {
        return None;
    }
    // SAFETY: bounded index and static storage lifetime.
    let base = unsafe { core::ptr::addr_of!(SECONDARY_HART_STACKS[idx - 1]) as usize };
    Some(base + SECONDARY_STACK_SIZE)
}

/// Boot gate (self-diagnosing evidence): one line per expected hart with the
/// full bring-up picture — start error, online bit, reached stage, HSM state.
/// Emitted unconditionally on SMP>=2 boots so ANY failure localizes itself
/// without re-instrumented reruns.
pub fn emit_bringup_gate(expected_mask: usize) {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        let online = cpu_online_mask();
        if expected_mask.count_ones() <= 1 {
            return;
        }
        for (idx, stage) in BRINGUP_STAGE.iter().enumerate().skip(1) {
            let bit = 1usize << idx;
            if expected_mask & bit == 0 {
                continue;
            }
            let status = sbi::hart_get_status(idx);
            log_info!(
                target: "smp",
                "KGATE: smp hart{} online={} stage={} hsm={}",
                idx,
                (online & bit != 0) as usize,
                stage.load(Ordering::Acquire),
                status.value
            );
        }
        if online & expected_mask == expected_mask {
            log_info!(target: "smp", "KGATE: smp bringup ok mask=0x{:x}", online);
        } else {
            log_error!(
                target: "smp",
                "KGATE: smp bringup DEGRADED expected=0x{:x} got=0x{:x}",
                expected_mask,
                online
            );
        }
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    let _ = expected_mask;
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

            // Prepare the hart-local block BEFORE the hart can start executing:
            // hart_start is asynchronous, and the secondary's trap install
            // reads this block.
            hart_local_prepare(cpu, stack_top);

            let ret = sbi::hart_start(hart.as_index(), __secondary_hart_start as usize, stack_top);
            match ret.error {
                0 | SBI_ERR_ALREADY_AVAILABLE | SBI_ERR_ALREADY_STARTED => {
                    expected_mask |= 1usize << idx;
                    // SEQUENTIAL bring-up: wait for this hart to come online
                    // before starting the next. Batched hart_start requests
                    // were observed to lose a hart nondeterministically (HSM
                    // reports STARTED but the hart never reaches the kernel
                    // entry; restart returns ALREADY_AVAILABLE).
                    if !wait_for_online_mask(1usize << idx, 500_000_000) {
                        log_error!(
                            target: "smp",
                            "KINIT: hart{} did not come online (sequential bring-up)",
                            idx
                        );
                    }
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

/// Bounded bring-up retry: re-issues `hart_start` for every expected hart
/// that has not come online (HSM start requests issued in quick succession
/// were observed to get lost nondeterministically — a hart never reached the
/// kernel entry despite a success return). Logs the SBI HSM status of each
/// missing hart for the evidence trail.
pub fn retry_missing_harts(expected_mask: usize) {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        let online = cpu_online_mask();
        for (idx, stage) in BRINGUP_STAGE.iter().enumerate().skip(1) {
            let bit = 1usize << idx;
            if expected_mask & bit == 0 || online & bit != 0 {
                continue;
            }
            let hart = HartId::from_raw(idx as u16);
            let cpu = CpuId::from_hart(hart);
            let status = sbi::hart_get_status(hart.as_index());
            log_error!(
                target: "smp",
                "KINIT: hart{} missing (stage={} hsm_err=0x{:x} hsm_state={}) — retrying start",
                idx,
                stage.load(Ordering::Acquire),
                status.error,
                status.value
            );
            let Some(stack_top) = secondary_stack_top(cpu) else {
                continue;
            };
            hart_local_prepare(cpu, stack_top);
            let ret = sbi::hart_start(hart.as_index(), __secondary_hart_start as usize, stack_top);
            log_info!(target: "smp", "KINIT: hart{} restart err=0x{:x}", idx, ret.error);
        }
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    let _ = expected_mask;
}

/// Waits (bounded by TIME, not iterations) until every expected hart is
/// online. Iteration budgets are meaningless across icount/MTTCG — 2M
/// spin_loops are microseconds under MTTCG, which flakily timed out hart 3
/// on SMP=4 bring-up.
pub fn wait_for_online_mask(expected_mask: usize, budget_ns: u64) -> bool {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        // QEMU virt mtime runs at 10 MHz (100ns/tick).
        let deadline = (riscv::register::time::read() as u64).saturating_add(budget_ns / 100);
        loop {
            if cpu_online_mask() & expected_mask == expected_mask {
                return true;
            }
            if (riscv::register::time::read() as u64) >= deadline {
                return false;
            }
            core::hint::spin_loop();
        }
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        let _ = budget_ns;
        cpu_online_mask() & expected_mask == expected_mask
    }
}
