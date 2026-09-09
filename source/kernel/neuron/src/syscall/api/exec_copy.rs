// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Phase-B executor for the staged exec copy plan (`exec::CopyPlan`):
//! the byte moves that run with the BKL DROPPED after `exec_phase_a` /
//! `exec_v2_phase_a` validated and staged them. On the OS target the moves
//! are chunked and service the TLB-shootdown mailbox between chunks
//! (`smp::tlb::{zero,copy}_bytes_polled`) — SIE is off in trap context, so a
//! shootdown IPI cannot interrupt a long memset/copy otherwise.
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Internal

#[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
use core::ptr;

use super::exec::CopyPlan;

/// Execute a staged plan (phase B — caller holds NO kernel locks).
pub(crate) fn run_copy_plan(plan: &CopyPlan) {
    for op in plan.ops.iter().take(plan.len) {
        if op.len == 0 {
            continue;
        }
        // SAFETY: the plan was staged under the BKL from validated ranges the
        // new image owns exclusively until it is resumed.
        #[cfg(all(target_arch = "riscv64", target_os = "none"))]
        unsafe {
            if op.src == usize::MAX {
                crate::smp::tlb::zero_bytes_polled(op.dst as *mut u8, op.len);
            } else {
                crate::smp::tlb::copy_bytes_polled(op.src as *const u8, op.dst as *mut u8, op.len);
            }
        }
        #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
        unsafe {
            if op.src == usize::MAX {
                ptr::write_bytes(op.dst as *mut u8, 0, op.len);
            } else {
                ptr::copy_nonoverlapping(op.src as *const u8, op.dst as *mut u8, op.len);
            }
        }
    }
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    if plan.fence_i {
        unsafe {
            core::arch::asm!("fence.i", options(nostack));
        }
    }
}
