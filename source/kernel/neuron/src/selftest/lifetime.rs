// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Boot-time proofs of "an object dies with its last capability" and "a death is an
//! event" (RFC-0100 §1, TASK-0333 6a), against the real tables and the real syscall table:
//! a non-blocking EOF-opted receiver learns of its last peer's death (the closed window's
//! app-host parked forever before this, measured on the board); a send into an endpoint no
//! RECV capability names returns `PeerClosed`; a task's exit frees the timers, waitsets,
//! fences and endpoints it owned and the endpoint only it named; a child's exit writes one
//! `OP_CHILD_EXITED` frame into the endpoint its parent bound.
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the `KSELFTEST:` lines below, every QEMU lane (`scripts/qemu-test.sh`)

use super::Context;
use crate::cap::{Capability, CapabilityKind, Rights};
use crate::ipc::{header::MessageHeader, IpcError, Message, Payload};
use crate::syscall::{
    api, Args, Error as SysError, SyscallTable, SYSCALL_CAP_CLOSE, SYSCALL_CHILD_EXIT_BIND,
    SYSCALL_IPC_RECV_V1,
};
use crate::task::{ExitReason, Pid};

/// Bootstrap cap slots above the waitset selftest's (49..51).
const RECV_SLOT: usize = 52;
const SEND_SLOT: usize = 53;
const NOTIFY_SLOT: usize = 54;
/// `IPC_SYS_NONBLOCK | IPC_SYS_EOF`.
const RECV_NONBLOCK_EOF: usize = 1 | 4;
/// A user-range address the empty-queue path never writes to (the check is a range check).
const USER_SCRATCH: usize = 0x1000;

fn frame(ep: u32) -> Message {
    let payload = [0xa5u8; 4];
    let header = MessageHeader::new(0, ep, 1, 0, payload.len() as u32);
    Message::new(header, Payload::from_slice(&payload), None)
}

fn give(ctx: &mut Context<'_>, pid: Pid, slot: usize, ep: u32, rights: Rights) -> bool {
    let cap = Capability { kind: CapabilityKind::Endpoint(ep), rights };
    ctx.tasks.caps_of_mut(pid).is_some_and(|caps| caps.set(slot, cap).is_ok())
}

pub(super) fn run_lifetime_selftests(ctx: &mut Context<'_>) {
    run_eof_nonblocking(ctx);
    run_peer_closed_send(ctx);
    run_exit_releases(ctx);
    run_child_exit_notify(ctx);
    ctx.tasks.set_current(Pid::KERNEL);
}

/// A receiver (the endpoint's owner) whose one sender wrote once and then closed its SEND
/// capability gets `PeerClosed` from a NON-BLOCKING, EOF-opted recv — the waitset shape.
fn run_eof_nonblocking(ctx: &mut Context<'_>) {
    ctx.tasks.set_current(Pid::KERNEL);
    let receiver = ctx.tasks.selftest_create_dummy_task(Pid::KERNEL, ctx.scheduler);
    let sender = ctx.tasks.selftest_create_dummy_task(Pid::KERNEL, ctx.scheduler);
    let Ok(ep) = ctx.router.create_endpoint(4, Some(receiver.as_raw())) else {
        log_error!(target: "selftest", "KSELFTEST: ipc eof nonblocking FAIL (create)");
        return;
    };
    if !give(ctx, receiver, RECV_SLOT, ep, Rights::RECV)
        || !give(ctx, sender, SEND_SLOT, ep, Rights::SEND)
    {
        log_error!(target: "selftest", "KSELFTEST: ipc eof nonblocking FAIL (caps)");
        return;
    }
    // The sender was observed once (the latch), the receiver drained the frame.
    let _ = ctx.router.send(ep, frame(ep));
    let _ = ctx.router.recv(ep);
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
    // The sender closes its SEND capability: the last peer is gone.
    sys_ctx.tasks.set_current(sender);
    let closed =
        table.dispatch(SYSCALL_CAP_CLOSE, &mut sys_ctx, &Args::new([SEND_SLOT, 0, 0, 0, 0, 0]));
    sys_ctx.tasks.set_current(receiver);
    let args = Args::new([RECV_SLOT, USER_SCRATCH, USER_SCRATCH, 0, RECV_NONBLOCK_EOF, 0]);
    let verdict = table.dispatch(SYSCALL_IPC_RECV_V1, &mut sys_ctx, &args);
    sys_ctx.tasks.set_current(Pid::KERNEL);
    match (closed, verdict) {
        (Ok(_), Err(SysError::Ipc(IpcError::PeerClosed))) => {
            log_info!(target: "selftest", "KSELFTEST: ipc eof nonblocking ok");
        }
        (closed, verdict) => log_error!(
            target: "selftest",
            "KSELFTEST: ipc eof nonblocking FAIL (close={:?} recv={:?})",
            closed,
            verdict
        ),
    }
}

/// Once the only RECV capability to an endpoint is closed, a send into it returns
/// `PeerClosed` — nobody can ever drain it, so no sender parks on it.
fn run_peer_closed_send(ctx: &mut Context<'_>) {
    ctx.tasks.set_current(Pid::KERNEL);
    let receiver = ctx.tasks.selftest_create_dummy_task(Pid::KERNEL, ctx.scheduler);
    let sender = ctx.tasks.selftest_create_dummy_task(Pid::KERNEL, ctx.scheduler);
    let Ok(ep) = ctx.router.create_endpoint(4, Some(receiver.as_raw())) else {
        log_error!(target: "selftest", "KSELFTEST: ipc peer closed send FAIL (create)");
        return;
    };
    if !give(ctx, receiver, RECV_SLOT, ep, Rights::RECV)
        || !give(ctx, sender, SEND_SLOT, ep, Rights::SEND)
    {
        log_error!(target: "selftest", "KSELFTEST: ipc peer closed send FAIL (caps)");
        return;
    }
    let before = ctx.router.send(ep, frame(ep)).is_ok();
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
    sys_ctx.tasks.set_current(receiver);
    let closed =
        table.dispatch(SYSCALL_CAP_CLOSE, &mut sys_ctx, &Args::new([RECV_SLOT, 0, 0, 0, 0, 0]));
    sys_ctx.tasks.set_current(Pid::KERNEL);
    let after = ctx.router.send(ep, frame(ep));
    match (before, closed, after) {
        (true, Ok(_), Err(IpcError::PeerClosed)) => {
            log_info!(target: "selftest", "KSELFTEST: ipc peer closed send ok");
        }
        (before, closed, after) => log_error!(
            target: "selftest",
            "KSELFTEST: ipc peer closed send FAIL (before={} close={:?} after={:?})",
            before,
            closed,
            after
        ),
    }
}

/// A task's exit frees the timer, waitset and fence it owned, the endpoint it owned, and the
/// endpoint only it named (owned by its parent, which holds no capability to it).
fn run_exit_releases(ctx: &mut Context<'_>) {
    ctx.tasks.set_current(Pid::KERNEL);
    let parent = ctx.tasks.selftest_create_dummy_task(Pid::KERNEL, ctx.scheduler);
    let child = ctx.tasks.selftest_create_dummy_task(parent, ctx.scheduler);
    let (endpoints, timers, waitsets, fences) = (
        ctx.router.live_endpoint_count(),
        ctx.hart_timers.len(),
        ctx.waitsets.len(),
        ctx.fences.len(),
    );
    let (Ok(owned), Ok(named)) = (
        ctx.router.create_endpoint(4, Some(child.as_raw())),
        ctx.router.create_endpoint(4, Some(parent.as_raw())),
    ) else {
        log_error!(target: "selftest", "KSELFTEST: task exit releases FAIL (create)");
        return;
    };
    let objects = ctx.hart_timers.alloc(child.as_raw(), owned, 0).is_ok()
        && ctx.waitsets.alloc(child.as_raw()).is_ok()
        && ctx.fences.alloc(child.as_raw()).is_ok()
        && give(ctx, child, RECV_SLOT, owned, Rights::RECV | Rights::SEND | Rights::MANAGE)
        && give(ctx, child, SEND_SLOT, named, Rights::SEND);
    if !objects {
        log_error!(target: "selftest", "KSELFTEST: task exit releases FAIL (objects)");
        return;
    }
    ctx.tasks.set_current(child);
    api::exit_current_and_release(
        ctx.tasks,
        ctx.router,
        ctx.scheduler,
        api::ExitTables {
            hart_timers: ctx.hart_timers,
            waitsets: ctx.waitsets,
            fences: ctx.fences,
        },
        0,
        ExitReason::Voluntary,
    );
    ctx.tasks.set_current(Pid::KERNEL);
    let after = (
        ctx.router.live_endpoint_count(),
        ctx.hart_timers.len(),
        ctx.waitsets.len(),
        ctx.fences.len(),
    );
    let dead = !ctx.router.endpoint_alive(owned) && !ctx.router.endpoint_alive(named);
    if dead && after == (endpoints, timers, waitsets, fences) {
        log_info!(
            target: "selftest",
            "KSELFTEST: task exit releases ok (endpoints=2 timers=1 waitsets=1 fences=1)"
        );
    } else {
        log_error!(
            target: "selftest",
            "KSELFTEST: task exit releases FAIL (dead={} before={:?} after={:?})",
            dead,
            (endpoints, timers, waitsets, fences),
            after
        );
    }
}

/// A parent that bound an endpoint for its children's deaths finds one `OP_CHILD_EXITED`
/// frame there after a child's exit: the pid, the code, the reason word.
fn run_child_exit_notify(ctx: &mut Context<'_>) {
    ctx.tasks.set_current(Pid::KERNEL);
    let parent = ctx.tasks.selftest_create_dummy_task(Pid::KERNEL, ctx.scheduler);
    let child = ctx.tasks.selftest_create_dummy_task(parent, ctx.scheduler);
    let Ok(notify) = ctx.router.create_endpoint(4, Some(parent.as_raw())) else {
        log_error!(target: "selftest", "KSELFTEST: child exit notify FAIL (create)");
        return;
    };
    if !give(ctx, parent, NOTIFY_SLOT, notify, Rights::RECV | Rights::SEND | Rights::MANAGE) {
        log_error!(target: "selftest", "KSELFTEST: child exit notify FAIL (caps)");
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
    sys_ctx.tasks.set_current(parent);
    let bound = table.dispatch(
        SYSCALL_CHILD_EXIT_BIND,
        &mut sys_ctx,
        &Args::new([NOTIFY_SLOT, 0, 0, 0, 0, 0]),
    );
    sys_ctx.tasks.set_current(child);
    api::exit_current_and_release(
        sys_ctx.tasks,
        sys_ctx.router,
        sys_ctx.scheduler,
        api::ExitTables {
            hart_timers: sys_ctx.hart_timers,
            waitsets: sys_ctx.waitsets,
            fences: sys_ctx.fences,
        },
        7,
        ExitReason::Voluntary,
    );
    sys_ctx.tasks.set_current(Pid::KERNEL);
    let heard = sys_ctx.router.recv(notify).ok().and_then(|msg| {
        let p = msg.payload.as_slice();
        (p.len() == 13 && p[0] == api::OP_CHILD_EXITED).then(|| {
            (
                u32::from_le_bytes([p[1], p[2], p[3], p[4]]),
                i32::from_le_bytes([p[5], p[6], p[7], p[8]]),
                u32::from_le_bytes([p[9], p[10], p[11], p[12]]),
            )
        })
    });
    match (bound, heard) {
        (Ok(_), Some((pid, 7, 0))) if pid == child.as_raw() => {
            log_info!(
                target: "selftest",
                "KSELFTEST: child exit notify ok (pid={} code=7)",
                pid
            );
        }
        (bound, heard) => log_error!(
            target: "selftest",
            "KSELFTEST: child exit notify FAIL (bind={:?} heard={:?})",
            bound,
            heard
        ),
    }
}
