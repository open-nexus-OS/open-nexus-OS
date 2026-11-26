// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the trap's entry of the IPC syscalls, before the kernel lock (TASK-0330 P2,
//! ADR-0102). Send, receive, call, reply, a waitset's wait and a fence's wait and signal are
//! one body each over `syscall::api::ipc_ctx::Ipc` — the caller's scheduling record, the
//! router and the two tables, each behind its own lock. Since P3 a mapping, an unmap and a
//! device's runs enter here too: the caller's address space and the memory object are behind
//! their own locks as well; since P4 the copies into and out of an object (`vmo_write`,
//! `vmo_read`), `yield`, a capability's clone and close (an endpoint's death included), a
//! timer's set and cancel and a console byte do (`ipc_ctx::body` is the list). A user
//! thread's syscall runs that body
//! here; the kernel lock is taken only when the body asks for it (a page of the caller's not
//! ready, an order waiting for it), and then the whole syscall re-executes under it.
//!
//! How it ends: a return value goes into the thread's saved frame and the trap returns to it —
//! or, a task that this syscall woke going ahead of it (ADR-0096), the thread is requeued and
//! the hart returns into its scheduler loop, whose pick is the switch. A blocked thread's hart
//! returns into its loop as well; from the block on, the record is whoever wakes it.
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: every QEMU lane's IPC ladder (`SELFTEST: ipc …`, `KSELFTEST: ipc call
//!   budget`), the rank-check lane; the bodies' parts on the host (`sync::wait_word`,
//!   `user_walk`, `ipc`)
//! INVARIANTS: the frame is saved into the record before the body runs and is complete before
//!   the thread can leave this hart; after a block or a requeue this hart touches neither, and
//!   is on the kernel's own space from the hold that gave the thread up (`mm::hart_spaces`)
//! ADR: docs/adr/0102-a-thread-blocks-in-two-steps-and-a-waker-never-waits-ipc-leaves-the-kernel-lock.md

use super::errno::encode_error;
use super::*;
use crate::syscall::api::ipc_ctx::{self, Ipc, Outcome};
use crate::syscall::Args;
use crate::task::Pid;

/// The objects an IPC body works on: `kmain`'s, for the kernel's whole life.
struct Objects {
    router: NonNull<ipc::Router>,
    waitsets: NonNull<crate::waitset::WaitsetTable>,
    fences: NonNull<crate::fence::FenceTable>,
    timer: &'static dyn Timer,
    spaces: NonNull<AddressSpaceManager>,
    timers: NonNull<crate::timer::HartTimers>,
}

// SAFETY: the objects are reached through shared references only (the kernel lock's
// holder gets the same, `KernelGuard::parts`), each guards its state with its own locks, and
// `kmain` never drops them; the timer reads the hart's clock.
unsafe impl Send for Objects {}
// SAFETY: as above.
unsafe impl Sync for Objects {}

static OBJECTS: spin::Once<Objects> = spin::Once::new();

/// Names the objects, once, when the trap runtime is installed.
pub(super) fn install(
    router: &ipc::Router,
    waitsets: &crate::waitset::WaitsetTable,
    fences: &crate::fence::FenceTable,
    timer: &'static dyn Timer,
    spaces: &AddressSpaceManager,
    timers: &crate::timer::HartTimers,
) {
    OBJECTS.call_once(|| Objects {
        router: NonNull::from(router),
        waitsets: NonNull::from(waitsets),
        fences: NonNull::from(fences),
        timer,
        spaces: NonNull::from(spaces),
        timers: NonNull::from(timers),
    });
}

/// The timer caps that fired, delivered by this hart under no kernel lock (TASK-0330 P4): the
/// idle loop's, for a hart with nothing else due. How many fired; `None` before the runtime.
pub(crate) fn deliver_timers() -> Option<usize> {
    let objects = OBJECTS.get()?;
    // SAFETY: `install` named live objects that `kmain` never drops; shared references only,
    // each object behind its own lock.
    let (timers, router) = unsafe { (objects.timers.as_ref(), objects.router.as_ref()) };
    let mut queues = crate::sched::Scheduler::unlocked();
    Some(process_expired_timers(objects.timer, timers, router, &mut queues))
}

/// Serves the user syscall in `frame` without the kernel lock, if it is an IPC syscall and
/// nothing asks for the lock first. `true`: `frame` is what the trap returns to — the thread
/// with its result, or this hart's scheduler loop. `false`: untouched, the locked path takes it.
#[cfg(all(target_arch = "riscv64", target_os = "none"))]
pub(super) fn serve(frame: &mut TrapFrame) -> bool {
    const SSTATUS_SPP: usize = 1 << 8;
    let Some(body) = ipc_ctx::body(frame.x[17]) else {
        return false;
    };
    // A kernel-mode task and a hart before its scheduler loop are resumed through the locked
    // path's epilogue alone; a pressure level waits for a holder of the lock to deliver it.
    let me = crate::sched::runs_here();
    if frame.sstatus & SSTATUS_SPP != 0
        || me == Pid::KERNEL
        || !crate::cpu_main::sched_loop_entered(crate::smp::cpu_current_id())
        || crate::mm::pressure_endpoint::pending()
    {
        return false;
    }
    let (Some(objects), Some(rec)) = (OBJECTS.get(), crate::sched::record::get(me)) else {
        return false;
    };
    // ADR-0082: an order left for this thread (a kill, a freeze) runs before its syscall does.
    if rec.space().is_none() || rec.pending().is_some() {
        return false;
    }
    // The thread's whole state, from here on: what a block leaves behind and a wake resumes.
    *rec.frame_mut() = *frame;
    record(frame);
    // SAFETY: `install` named live objects that `kmain` never drops; shared references only.
    let ipc = unsafe {
        Ipc {
            me,
            rec,
            router: objects.router.as_ref(),
            waitsets: objects.waitsets.as_ref(),
            fences: objects.fences.as_ref(),
            timer: objects.timer,
            timers: objects.timers.as_ref(),
            spaces: objects.spaces.as_ref(),
            free: true,
        }
    };
    let args =
        Args::new([frame.x[10], frame.x[11], frame.x[12], frame.x[13], frame.x[14], frame.x[15]]);
    let t0 = riscv::register::time::read() as u64;
    let out = body(&ipc, &args);
    assert!(crate::sync::ranked::holds_none(), "ranked lock held past an IPC syscall");
    super::budgets::note_long_call(t0, frame.x[17]);
    let (ret, yields) = match out {
        // Nothing is changed: the locked path runs the syscall whole.
        Ok(Outcome::Locked) => return false,
        Ok(Outcome::Blocked(_)) => {
            leave(frame);
            return true;
        }
        Ok(Outcome::Done(value)) => (value, false),
        Ok(Outcome::Yielded) => (0, true),
        Err(err) => (encode_error(err), false),
    };
    // The saved frame first — a body may have written registers there (a short reply) — and
    // complete before the thread can be queued: another hart may take it from the queue.
    let saved = rec.frame_mut();
    saved.x[10] = ret;
    saved.sepc = saved.sepc.wrapping_add(4);
    // Requeued, the thread is any hart's to take — and to kill and reap: this hart is off its
    // space in the hold that queues it.
    let off = crate::hart_spaces::enter_kernel_space;
    let mut order = crate::sched::Scheduler::unlocked();
    let gone = if yields { order.yield_if_waiting(off) } else { order.preempt_if_ready(off) };
    if gone {
        leave(frame);
    } else {
        *frame = *saved;
    }
    true
}

/// This hart runs its thread no more: it returns into its scheduler loop.
#[cfg(all(target_arch = "riscv64", target_os = "none"))]
#[track_caller]
fn leave(frame: &mut TrapFrame) {
    crate::task::set_current_here(Pid::KERNEL);
    crate::cpu_main::stage_idle_reentry_frame(frame);
}
