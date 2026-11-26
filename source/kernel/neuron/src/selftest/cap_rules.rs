// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: RFC-0112's capability rules at boot (amendment 2026-11-02), through the real syscall
//! table and tables. `KSELFTEST: cap transfer rules ok (`: a transfer into a stranger (a sibling)
//! is EPERM by both transfer syscalls while one into the caller's child lands; a REPLACE_DEAD
//! transfer re-pins a slot holding a dead endpoint's capability and an empty one, and refuses a
//! live capability, another kind, an occupied slot without the flag and an unknown flag bit.
//! `KSELFTEST: in-flight caps released ok`: a reply capability moved with a request the server
//! never read is released at the server's death — the caller's inbox learns its last sender is
//! gone, where it used to wait forever. `KSELFTEST: stale endpoint binds refused ok (`
//! (TASK-0330 P4): a bind through a capability that outlived its endpoint leaves no id the
//! dead slot's next tenant would receive, and a child-exit binding holds the slot.
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the `KSELFTEST:` lines below, every QEMU lane; the rules host-tested in
//!   `crate::cap_rules`

use super::Context;
use crate::cap::{CapError, Capability, CapabilityKind, Held, Rights};
use crate::ipc::{header::MessageHeader, IpcError, Message, Payload};
use crate::mm::AddressSpaceError;
use crate::syscall::{
    api, Args, Error as SysError, SyscallTable, SYSCALL_CAP_CLOSE, SYSCALL_CAP_TRANSFER,
    SYSCALL_CAP_TRANSFER_TO, SYSCALL_CHILD_EXIT_BIND, SYSCALL_MEM_PRESSURE_BIND,
    SYSCALL_TIMER_CREATE, SYSCALL_WAITSET_ADD, SYSCALL_WAITSET_CREATE,
};
use crate::task::{ExitReason, Pid, TransferError};

/// The transferring task's slot holding what it hands out.
const HELD: usize = 60;
/// The target slots in its child (one per case).
const PINNED: usize = 61;
const DEAD_SLOT: usize = 62;
const FLAGLESS_SLOT: usize = 63;
const KIND_SLOT: usize = 64;
const EMPTY_SLOT: usize = 65;
const FLAGS_SLOT: usize = 66;
/// The waiting client's slot of its inbox.
const INBOX_SLOT: usize = 60;
/// The binder's capability that outlived its endpoint, and its only one to a live endpoint.
const STALE_SLOT: usize = 60;
const LAST_SLOT: usize = 61;

/// A capability into `pid`'s `slot` through the doors every capability comes through
/// (RFC-0102): one to an endpoint is minted at the LIVE endpoint, a plain kind made as it is.
fn set(ctx: &mut Context<'_>, pid: Pid, slot: usize, kind: CapabilityKind, rights: Rights) -> bool {
    let held = match kind {
        CapabilityKind::Endpoint(id) => ctx.router.mint(id, rights).ok(),
        _ => Held::plain(Capability { kind, rights }).ok(),
    };
    held.is_some_and(|held| {
        ctx.tasks.caps_of_mut(pid).is_some_and(|mut caps| caps.set(slot, held).is_ok())
    })
}

/// A capability to the live endpoint `ep` into the current task's `slot` — minted at the
/// endpoint (the selftests' wiring of a task they drive through the syscall table).
pub(super) fn install_endpoint(
    ctx: &mut api::Context<'_>,
    slot: usize,
    ep: u32,
    rights: Rights,
) -> Result<(), SysError> {
    let held = ctx.router.mint(ep, rights)?;
    Ok(ctx.tasks.current_caps_mut().set(slot, held)?)
}

pub(super) fn run_cap_rules_selftests(ctx: &mut Context<'_>) {
    run_transfer_rules(ctx);
    run_in_flight_release(ctx);
    run_stale_binds(ctx);
    ctx.tasks.set_current(Pid::KERNEL);
}

/// The target rule and REPLACE_DEAD through both transfer syscalls.
fn run_transfer_rules(ctx: &mut Context<'_>) {
    ctx.tasks.set_current(Pid::KERNEL);
    let parent = ctx.tasks.selftest_create_dummy_task(Pid::KERNEL, ctx.scheduler);
    let child = ctx.tasks.selftest_create_dummy_task(parent, ctx.scheduler);
    let stranger = ctx.tasks.selftest_create_dummy_task(Pid::KERNEL, ctx.scheduler);
    let dead_owner = ctx.tasks.selftest_create_dummy_task(Pid::KERNEL, ctx.scheduler);
    let (Ok(live), Ok(dead)) = (
        ctx.router.create_endpoint(4, Some(parent.as_raw())),
        ctx.router.create_endpoint(4, Some(dead_owner.as_raw())),
    ) else {
        log_error!(target: "selftest", "KSELFTEST: cap transfer rules FAIL (create)");
        return;
    };
    // The capabilities to the endpoint that dies are made while it lives — a capability to a
    // dead endpoint exists only as one that outlived it.
    let staged = set(ctx, parent, HELD, CapabilityKind::Endpoint(live), Rights::SEND)
        && set(ctx, child, DEAD_SLOT, CapabilityKind::Endpoint(dead), Rights::SEND)
        && set(ctx, child, FLAGLESS_SLOT, CapabilityKind::Endpoint(dead), Rights::SEND)
        && set(ctx, child, KIND_SLOT, CapabilityKind::Timer(u32::MAX), Rights::empty());
    let _ = ctx.router.close_endpoints_for_owner(dead_owner.as_raw());
    if !staged {
        log_error!(target: "selftest", "KSELFTEST: cap transfer rules FAIL (stage)");
        return;
    }

    let mut table = SyscallTable::new();
    api::install_handlers(&mut table);
    let timer = ctx.hal.timer();
    let mut sys = api::Context::new(
        ctx.scheduler,
        ctx.tasks,
        ctx.router,
        ctx.address_spaces,
        timer,
        ctx.hart_timers,
        ctx.waitsets,
        ctx.fences,
    );
    sys.tasks.set_current(parent);
    let send = Rights::SEND.bits() as usize;
    let to = |pid: Pid, slot: usize, flags: usize| {
        Args::new([pid.as_index(), HELD, send, slot, flags, 0])
    };
    let denied = |r: Result<usize, SysError>| {
        matches!(r, Err(SysError::Transfer(TransferError::Capability(CapError::PermissionDenied))))
    };
    let replace = api::CAP_TRANSFER_REPLACE_DEAD;

    // A stranger — a sibling — through both transfer syscalls.
    let stranger_refused = usize::from(denied(table.dispatch(
        SYSCALL_CAP_TRANSFER_TO,
        &mut sys,
        &to(stranger, PINNED, 0),
    ))) + usize::from(denied(table.dispatch(
        SYSCALL_CAP_TRANSFER,
        &mut sys,
        &Args::new([stranger.as_index(), HELD, send, 0, 0, 0]),
    )));
    // The caller's own child.
    let child_pinned = table.dispatch(SYSCALL_CAP_TRANSFER_TO, &mut sys, &to(child, PINNED, 0));
    // REPLACE_DEAD over a dead endpoint's capability, and into an empty slot.
    let replaced =
        table.dispatch(SYSCALL_CAP_TRANSFER_TO, &mut sys, &to(child, DEAD_SLOT, replace)).is_ok()
            && sys.tasks.caps_of(child).and_then(|caps| caps.get(DEAD_SLOT).ok()).map(|c| c.kind)
                == Some(CapabilityKind::Endpoint(live));
    let empty_pinned =
        table.dispatch(SYSCALL_CAP_TRANSFER_TO, &mut sys, &to(child, EMPTY_SLOT, replace)).is_ok();
    // Refused: a live capability (PINNED holds `live` now), another kind, an occupied slot
    // without the flag, an unknown flag bit.
    let live_kept =
        denied(table.dispatch(SYSCALL_CAP_TRANSFER_TO, &mut sys, &to(child, PINNED, replace)));
    let kind_kept =
        denied(table.dispatch(SYSCALL_CAP_TRANSFER_TO, &mut sys, &to(child, KIND_SLOT, replace)));
    let flagless_kept =
        denied(table.dispatch(SYSCALL_CAP_TRANSFER_TO, &mut sys, &to(child, FLAGLESS_SLOT, 0)));
    let flags_refused = matches!(
        table.dispatch(SYSCALL_CAP_TRANSFER_TO, &mut sys, &to(child, FLAGS_SLOT, replace << 1)),
        Err(SysError::AddressSpace(AddressSpaceError::InvalidArgs))
    );
    sys.tasks.set_current(Pid::KERNEL);
    // Nothing of the staging stays named: the timer name has no object, the dead id goes free.
    if let Some(mut caps) = sys.tasks.caps_of_mut(child) {
        let _ = caps.take(KIND_SLOT);
        let _ = caps.take(FLAGLESS_SLOT);
    }

    let ok = stranger_refused == 2
        && child_pinned.is_ok()
        && replaced
        && empty_pinned
        && live_kept
        && kind_kept
        && flagless_kept
        && flags_refused;
    if ok {
        log_info!(
            target: "selftest",
            "KSELFTEST: cap transfer rules ok (stranger=2 child=1 replaced=2 live=1 kind=1 flagless=1 flags=1)"
        );
    } else {
        log_error!(
            target: "selftest",
            "KSELFTEST: cap transfer rules FAIL (stranger={} child={:?} replaced={} empty={} live={} kind={} flagless={} flags={})",
            stranger_refused,
            child_pinned,
            replaced,
            empty_pinned,
            live_kept,
            kind_kept,
            flagless_kept,
            flags_refused
        );
    }
}

/// A client's call is still queued when its server dies: the reply capability it moved — the
/// only SEND to the client's inbox — is released with the server's endpoints, so the inbox's
/// receiver learns its last sender is gone (the EOF latch a waitset and a recv end on).
fn run_in_flight_release(ctx: &mut Context<'_>) {
    ctx.tasks.set_current(Pid::KERNEL);
    let server = ctx.tasks.selftest_create_dummy_task(Pid::KERNEL, ctx.scheduler);
    let client = ctx.tasks.selftest_create_dummy_task(Pid::KERNEL, ctx.scheduler);
    let (Ok(request), Ok(inbox)) = (
        ctx.router.create_endpoint(4, Some(server.as_raw())),
        ctx.router.create_endpoint(1, Some(client.as_raw())),
    ) else {
        log_error!(target: "selftest", "KSELFTEST: in-flight caps released FAIL (create)");
        return;
    };
    let reply = ctx.router.mint(inbox, Rights::SEND).ok();
    let payload = [0x5au8; 4];
    let header = MessageHeader::new(0, request, 1, 0, payload.len() as u32);
    let queued = reply.is_some()
        && set(ctx, client, INBOX_SLOT, CapabilityKind::Endpoint(inbox), Rights::RECV)
        && ctx
            .router
            .send(request, Message::new(header, Payload::from_slice(&payload), reply))
            .is_ok();
    let before = ctx.router.eof_pending(inbox);
    ctx.tasks.set_current(server);
    api::exit_current_and_release(
        ctx.tasks,
        ctx.router,
        ctx.scheduler,
        api::ExitTables {
            hart_timers: ctx.hart_timers,
            waitsets: ctx.waitsets,
            fences: ctx.fences,
            address_spaces: ctx.address_spaces,
        },
        0,
        ExitReason::Voluntary,
    );
    ctx.tasks.set_current(Pid::KERNEL);
    let after = ctx.router.endpoint_alive(inbox) && ctx.router.eof_pending(inbox);
    if queued && !before && after {
        log_info!(target: "selftest", "KSELFTEST: in-flight caps released ok");
    } else {
        log_error!(
            target: "selftest",
            "KSELFTEST: in-flight caps released FAIL (queued={} before={} after={})",
            queued,
            before,
            after
        );
    }
}

/// TASK-0330 P4: an endpoint dies off the kernel lock, beside the locked paths that store its
/// id. A bind through a capability that outlived its endpoint leaves nothing that outlives the
/// call — a timer is refused, the pressure sink refused, a waitset member dropped (each would
/// reach the dead slot's next tenant); a child-exit binding holds the dead slot until it is
/// cleared; the close of a live endpoint's last capability kills it with the timer it fed.
fn run_stale_binds(ctx: &mut Context<'_>) {
    ctx.tasks.set_current(Pid::KERNEL);
    let binder = ctx.tasks.selftest_create_dummy_task(Pid::KERNEL, ctx.scheduler);
    let dead_owner = ctx.tasks.selftest_create_dummy_task(Pid::KERNEL, ctx.scheduler);
    let (Ok(dead), Ok(live)) = (
        ctx.router.create_endpoint(4, Some(dead_owner.as_raw())),
        ctx.router.create_endpoint(4, Some(binder.as_raw())),
    ) else {
        log_error!(target: "selftest", "KSELFTEST: stale endpoint binds refused FAIL (create)");
        return;
    };
    let both = Rights::SEND | Rights::RECV;
    let staged = set(ctx, binder, STALE_SLOT, CapabilityKind::Endpoint(dead), both)
        && set(ctx, binder, LAST_SLOT, CapabilityKind::Endpoint(live), both);
    let _ = ctx.router.close_endpoints_for_owner(dead_owner.as_raw());
    let sink_free = crate::mm::pressure_endpoint::holder().is_none();
    if !staged {
        log_error!(target: "selftest", "KSELFTEST: stale endpoint binds refused FAIL (stage)");
        return;
    }

    let mut table = SyscallTable::new();
    api::install_handlers(&mut table);
    let timer = ctx.hal.timer();
    let mut sys = api::Context::new(
        ctx.scheduler,
        ctx.tasks,
        ctx.router,
        ctx.address_spaces,
        timer,
        ctx.hart_timers,
        ctx.waitsets,
        ctx.fences,
    );
    sys.tasks.set_current(binder);
    let slot = |slot: usize| Args::new([slot, 0, 0, 0, 0, 0]);
    let timers = sys.hart_timers.len();
    let timer_refused = matches!(
        table.dispatch(SYSCALL_TIMER_CREATE, &mut sys, &slot(STALE_SLOT)),
        Err(SysError::Ipc(IpcError::NoSuchEndpoint))
    ) && sys.hart_timers.len() == timers;
    let sink_refused =
        table.dispatch(SYSCALL_MEM_PRESSURE_BIND, &mut sys, &slot(STALE_SLOT)).is_err()
            && crate::mm::pressure_endpoint::binding().is_none();
    let ws = table.dispatch(SYSCALL_WAITSET_CREATE, &mut sys, &slot(0)).ok();
    let added = ws.is_some_and(|ws| {
        let add = Args::new([ws, STALE_SLOT, 0, 0, 0, 0]);
        table.dispatch(SYSCALL_WAITSET_ADD, &mut sys, &add).is_ok()
    });
    // Asked again, nothing names it: the add's own check forgot it.
    let member_dropped = added && sys.waitsets.forget_endpoint(dead) == 0;
    // The binding holds the dead slot past the binder's last capability, until it is cleared.
    let bound = table.dispatch(SYSCALL_CHILD_EXIT_BIND, &mut sys, &slot(STALE_SLOT)).is_ok();
    let _ = sys.tasks.current_caps_mut().take(STALE_SLOT);
    let held = sys.router.revive_endpoint(dead, 4, None).is_err();
    let cleared = table.dispatch(SYSCALL_CHILD_EXIT_BIND, &mut sys, &slot(usize::MAX)).is_ok();
    let freed = sys.router.revive_endpoint(dead, 4, None) == Ok(dead);
    if freed {
        // Dead again and free: the slot as the staging found it.
        let _ = sys.router.close_endpoint(dead);
    }
    // The last capability of a live endpoint, closed: the endpoint dies, its timer with it.
    let fed = table.dispatch(SYSCALL_TIMER_CREATE, &mut sys, &slot(LAST_SLOT)).ok();
    let closed = table.dispatch(SYSCALL_CAP_CLOSE, &mut sys, &slot(LAST_SLOT)).is_ok();
    let died = fed.is_some()
        && closed
        && !sys.router.endpoint_alive(live)
        && sys.hart_timers.forget_endpoint(live) == 0
        && sys.hart_timers.len() == timers;
    // What the binder still names goes too: the waitset, and the timer's name (no object).
    for named in [ws, fed].into_iter().flatten() {
        let _ = table.dispatch(SYSCALL_CAP_CLOSE, &mut sys, &slot(named));
    }
    sys.tasks.set_current(Pid::KERNEL);

    let ok = timer_refused
        && sink_free
        && sink_refused
        && member_dropped
        && bound
        && held
        && cleared
        && freed
        && died;
    if ok {
        log_info!(
            target: "selftest",
            "KSELFTEST: stale endpoint binds refused ok (timer=1 sink=1 member=1 exit_bind=1 last_close=1)"
        );
    } else {
        log_error!(
            target: "selftest",
            "KSELFTEST: stale endpoint binds refused FAIL (timer={} sink_free={} sink={} member={} bound={} held={} cleared={} freed={} died={})",
            timer_refused,
            sink_free,
            sink_refused,
            member_dropped,
            bound,
            held,
            cleared,
            freed,
            died
        );
    }
}
