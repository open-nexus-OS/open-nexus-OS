// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: how a task the scheduler loop picked starts running (split out of `cpu_main.rs`,
//! which keeps the loop): the dispatch that takes no kernel lock (TASK-0363 P2 S3), the first-
//! dispatch proof of a secondary hart, and the context switch itself.
//! OWNERS: @kernel-sched-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: QEMU SMP lanes (`KSELFTEST: smp exec cpuN ok`, every user dispatch), the board
//!   ladder; the owner rule it relies on is host-tested (`sched/run_queue_tests.rs`)
//! INVARIANTS: a task dispatched here is held by this hart from its pick on
//!   (`run_queue::Owner::Running`); no lock is held across the switch

use crate::{task::Pid, types::CpuId};

/// Dispatches `pid`, just picked from this hart's queue, without the kernel lock. Its frame,
/// floating-point states and space are its scheduling record's, which this hart holds from the
/// pick on (`run_queue::Owner::Running`): nobody else touches them, and a kill or a freeze finds
/// it running here and leaves a pending order (`Scheduler::withdraw`). `None`: the record cannot
/// dispatch it — no space recorded, or a frame that is not a user one — and the locked path
/// takes it, already picked.
pub(super) fn dispatch_unlocked(cpu: CpuId, pid: Pid) -> Option<*const crate::trap::TrapFrame> {
    const SSTATUS_SPP: usize = 1 << 8;
    let record = crate::sched::record::get(pid)?;
    let view = record.space()?;
    let frame = record.frame();
    if frame.sstatus & SSTATUS_SPP != 0 || crate::phys::is_kernel_va(frame.sepc) {
        return None;
    }
    crate::sched::set_runs_here(pid);
    crate::trap::fp::now_runs(Some(record.fp()));
    let staged = crate::smp::hart_local_stage_resume(cpu, frame);
    crate::hart_spaces::enter_space(view);
    crate::smp::record_user_dispatch(cpu);
    note_first_user_dispatch(cpu);
    Some(staged)
}

/// A3 proof: emitted by a secondary hart itself on its FIRST user dispatch (event-anchored; a
/// boot-hart poll can miss the harness window).
pub(super) fn note_first_user_dispatch(cpu: CpuId) {
    if !cpu.is_boot()
        && crate::smp::hart_state::of(cpu).is_some_and(|hart| {
            hart.exec_proof_emitted.swap(1, core::sync::atomic::Ordering::AcqRel) == 0
        })
    {
        log_info!(target: "smp", "KSELFTEST: smp exec cpu{} ok", cpu.as_index());
    }
}

/// Pure assembly context switch - loads TrapFrame and executes sret.
/// This function never returns - it jumps to the task's saved PC.
/// CRITICAL: #[inline(always)] ensures the asm! block is inlined directly into the caller.
///
/// No trap may land between `csrw sepc` and `sret`: the scheduler loop runs with supervisor
/// interrupts ON, and a trap taken in that window writes ITS pc into `sepc` — the task then
/// returns to a kernel address in U-mode (QEMU 2026-10-23: netstackd, `INST @ sepc=` this very
/// asm, pid 0x31, killed with EFAULT). So the first instruction masks SIE, and the frame's
/// `sstatus` is written with SIE still clear; `sret` enables the task's interrupts from SPIE.
#[inline(always)]
pub(super) unsafe fn context_switch_to_task(frame: &crate::trap::TrapFrame) -> ! {
    // CRITICAL: This function is inline(always), so it's inserted directly after activate()
    // NO Rust code here - only raw pointer conversion and direct assembly jump
    let frame_ptr = frame as *const crate::trap::TrapFrame;

    // Direct jump to assembly - no logs, no temporaries, no cleanup code possible
    unsafe {
        core::arch::asm!(
        // Mask supervisor interrupts until the sret (SIE is bit 1).
        "csrci sstatus, 2",
        // Set up CSRs (sepc, sstatus — SIE kept clear; sret takes it from SPIE)
        "ld t0, {off_sepc}(t3)",
        "csrw sepc, t0",
        "ld t1, {off_sstatus}(t3)",
        "andi t1, t1, -3",
        "csrw sstatus, t1",

        // Load GPRs from frame (skip x0)
        "ld ra,   1*8(t3)",
        "ld gp,   3*8(t3)",
        "ld tp,   4*8(t3)",
        "ld t0,   5*8(t3)",
        "ld t1,   6*8(t3)",
        "ld t2,   7*8(t3)",
        "ld s0,   8*8(t3)",
        "ld s1,   9*8(t3)",
        "ld a0,  10*8(t3)",
        "ld a1,  11*8(t3)",
        "ld a2,  12*8(t3)",
        "ld a3,  13*8(t3)",
        "ld a4,  14*8(t3)",
        "ld a5,  15*8(t3)",
        "ld a6,  16*8(t3)",
        "ld a7,  17*8(t3)",
        "ld s2,  18*8(t3)",
        "ld s3,  19*8(t3)",
        "ld s4,  20*8(t3)",
        "ld s5,  21*8(t3)",
        "ld s6,  22*8(t3)",
        "ld s7,  23*8(t3)",
        "ld s8,  24*8(t3)",
        "ld s9,  25*8(t3)",
        "ld s10, 26*8(t3)",
        "ld s11, 27*8(t3)",
        "ld t4,  29*8(t3)",
        "ld t5,  30*8(t3)",
        "ld t6,  31*8(t3)",
        // Load sp before restoring pointer register
        "ld sp,   2*8(t3)",
        // Restore pointer register itself LAST to avoid clobbering base early
        "ld t3,  28*8(t3)",

        // Return to task via sret
        "sret",

        in("t3") frame_ptr,
        off_sepc = const core::mem::offset_of!(crate::trap::TrapFrame, sepc),
        off_sstatus = const core::mem::offset_of!(crate::trap::TrapFrame, sstatus),
        options(noreturn)
        );
    }
}
