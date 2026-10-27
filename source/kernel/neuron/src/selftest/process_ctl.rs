// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Boot-time proof of the process orders (TASK-0333 6d, ADR-0082, RFC-0111) against
//! the real syscall table: the refusals first — `task_resume`, `task_suspend` and `task_kill`
//! of a task that is not the caller's child are `ESRCH`, `task_kill` of a child its parent did
//! not make `killable` is `EPERM` — then the freeze itself: a frozen child is `Suspended`, its
//! receive endpoint is marked, the frame that fits its queue is taken (delivered on the thaw)
//! and the one that would park the sender is refused with `Frozen`; and a frozen, killable
//! child is killed into a zombie. The freeze of a RUNNING child and its thaw are proven by the
//! service launcher's probe (`KSELFTEST: task freeze ok`, `SELFTEST: exec child freeze thaw ok`).
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `KSELFTEST: process ctl ok (…)`, every QEMU lane and the board ladder

use super::Context;
use crate::cap::{CapError, Capability, CapabilityKind, Rights};
use crate::ipc::{header::MessageHeader, IpcError, Message, Payload};
use crate::syscall::{
    api, Args, Error as SysError, SyscallTable, SYSCALL_TASK_KILL, SYSCALL_TASK_RESUME,
    SYSCALL_TASK_SUSPEND,
};
use crate::task::{Pid, TaskState};

/// Bootstrap cap slot above the lifetime selftests' (52..54).
const RECV_SLOT: usize = 55;

fn frame(ep: u32) -> Message {
    let payload = [0x6du8; 4];
    let header = MessageHeader::new(0, ep, 1, 0, payload.len() as u32);
    Message::new(header, Payload::from_slice(&payload), None)
}

fn order(
    table: &SyscallTable,
    ctx: &mut api::Context<'_>,
    number: usize,
    pid: Pid,
) -> Result<usize, SysError> {
    table.dispatch(number, ctx, &Args::new([pid.as_raw() as usize, 0, 0, 0, 0, 0]))
}

pub(super) fn run_process_ctl_selftests(ctx: &mut Context<'_>) {
    ctx.tasks.set_current(Pid::KERNEL);
    // The caller, its child (not yet killable) and a task that is nobody's child of the caller.
    let caller = ctx.tasks.selftest_create_dummy_task(Pid::KERNEL, ctx.scheduler);
    let child = ctx.tasks.selftest_create_dummy_task(caller, ctx.scheduler);
    let foreign = ctx.tasks.selftest_create_dummy_task(Pid::KERNEL, ctx.scheduler);
    let Ok(ep) = ctx.router.create_endpoint(1, Some(child.as_raw())) else {
        log_error!(target: "selftest", "KSELFTEST: process ctl FAIL (create)");
        return;
    };
    let cap = Capability { kind: CapabilityKind::Endpoint(ep), rights: Rights::RECV };
    if !ctx.tasks.caps_of_mut(child).is_some_and(|caps| caps.set(RECV_SLOT, cap).is_ok()) {
        log_error!(target: "selftest", "KSELFTEST: process ctl FAIL (caps)");
        return;
    }
    let mut table = SyscallTable::new();
    api::install_handlers(&mut table);
    let timer = ctx.hal.timer();
    let mut sys_ctx = api::Context::new(
        ctx.scheduler,
        ctx.tasks,
        ctx.router,
        ctx.address_spaces,
        timer,
        ctx.hart_timers,
        ctx.waitsets,
        ctx.fences,
    );
    let ctx = &mut sys_ctx;
    ctx.tasks.set_current(caller);

    // The refusals: not my child (three orders), my child but not killable.
    let not_mine = [SYSCALL_TASK_RESUME, SYSCALL_TASK_SUSPEND, SYSCALL_TASK_KILL]
        .iter()
        .filter(|n| order(&table, ctx, **n, foreign) == Err(SysError::InvalidTarget))
        .count();
    let unkillable = order(&table, ctx, SYSCALL_TASK_KILL, child)
        == Err(SysError::Capability(CapError::PermissionDenied));
    let reject = not_mine + usize::from(unkillable);

    // The freeze: one task parked, the receiver marked, the queue's room taken, the park refused.
    let suspended = order(&table, ctx, SYSCALL_TASK_SUSPEND, child) == Ok(1);
    let frozen = suspended
        && ctx
            .tasks
            .task(child)
            .is_some_and(|t| t.is_frozen() && t.state() == TaskState::Suspended)
        && ctx.router.endpoint_frozen(ep);
    let fits = ctx.router.send(ep, frame(ep)).is_ok();
    let refused = ctx.router.send(ep, frame(ep)) == Err(IpcError::Frozen);
    let frozen_send = fits && refused;

    // The kill of a frozen, killable child: a zombie, no hart involved.
    if let Some(task) = ctx.tasks.task_mut(child) {
        task.set_killable(true);
    }
    let killed = order(&table, ctx, SYSCALL_TASK_KILL, child) == Ok(0)
        && ctx.tasks.task(child).is_some_and(|t| t.state() == TaskState::Zombie && !t.is_frozen());
    ctx.tasks.set_current(Pid::KERNEL);

    if reject == 4 && frozen && frozen_send && killed {
        log_info!(
            target: "selftest",
            "KSELFTEST: process ctl ok (reject=4 frozen=1 frozen_send=1 killed=1)"
        );
    } else {
        log_error!(
            target: "selftest",
            "KSELFTEST: process ctl FAIL (reject={} frozen={} frozen_send={} killed={})",
            reject,
            frozen,
            frozen_send,
            killed
        );
    }
}
