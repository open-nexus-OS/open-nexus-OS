// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: user-mode cache maintenance is a right, not every task's (ADR-0079) — the trap side.
//! Every hart runs user mode with Zicbom off; a `cbo.clean`/`cbo.flush`/`cbo.inval` from user
//! mode then traps as an illegal instruction, and this module decides it under the kernel lock:
//! a process holding a non-coherent device's capability gets the right for its address space and
//! the instruction is retried with the hart's bits on (the lazy pattern of an FPU); anything else
//! is the process's fault. The rule itself is pure and host-tested (`crate::cbo_rule`).
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `cbo_rule` host tests (decode + decision); QEMU `SELFTEST: cache maintenance
//!   refused ok` (a child without the right dies with reason=fault); the board's drivers of
//!   non-coherent devices (their DMA breaks visibly without it)
//! ADR: docs/adr/0079-user-cache-maintenance-is-the-right-of-a-non-coherent-devices-capability.md

use super::*;
use crate::cbo_rule::{decide, Verdict};

/// A user-mode illegal instruction at `frame.sepc` (`stval` = the instruction bits when the hart
/// reports them, else 0): `true` = the process was granted cache maintenance and the trap returns
/// to the same instruction; `false` = the caller reports and kills as for any illegal instruction.
#[cfg(all(target_arch = "riscv64", target_os = "none"))]
pub(super) fn try_grant(frame: &TrapFrame, stval: usize) -> bool {
    // U-mode trap → this hart holds no BKL; safe to acquire (as `fill_user_fault`).
    let Ok(mut kernel) = KernelGuard::acquire() else { return false };
    let (_, tasks, _, spaces, ..) = kernel.parts();
    let pid = tasks.current_pid();
    let Some(handle) = tasks.current_task().address_space() else { return false };
    let holds_right = tasks.caps_of(pid).is_some_and(|caps| caps.holds_noncoherent_device());
    let Ok(space) = spaces.get_mut(handle) else { return false };
    let inst = if stval != 0 { stval as u32 } else { fetch_instruction(space, frame.sepc) };
    match decide(
        inst,
        crate::hal::platform::user_cache_maintenance_supported(),
        holds_right,
        crate::hal::platform::user_cache_maintenance_on(),
    ) {
        Verdict::Grant => {
            space.grant_cache_maintenance();
            crate::hal::platform::apply_user_cache_maintenance(true);
            log_info!(target: "mm", "KINIT: cache maintenance granted (pid={})", pid.as_raw());
            true
        }
        Verdict::Refuse => {
            log_error!(
                target: "mm",
                "KINIT: cache maintenance refused (pid={} inst=0x{:08x})",
                pid.as_raw(),
                inst
            );
            false
        }
        Verdict::NotCbo => false,
    }
}

/// The 32-bit instruction at `va` through the space's own page table (a hart may report 0 in
/// `stval`); 0 when a half is not mapped — 0 decodes as no `cbo`.
#[cfg(all(target_arch = "riscv64", target_os = "none"))]
fn fetch_instruction(space: &crate::mm::address_space::AddressSpace, va: usize) -> u32 {
    let half = |at: usize| -> Option<u32> {
        let pa = space.page_table().translate(at)?;
        // SAFETY: a mapped user page, read through the direct map; 2-byte aligned (`sepc`).
        Some(u32::from(unsafe {
            core::ptr::read_volatile(crate::phys::phys_to_virt(pa) as *const u16)
        }))
    };
    match (half(va), half(va.wrapping_add(2))) {
        (Some(lo), Some(hi)) => lo | (hi << 16),
        _ => 0,
    }
}
