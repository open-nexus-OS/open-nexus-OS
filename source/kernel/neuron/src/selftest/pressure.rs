// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Boot-time proof of the memory pressure sink (TASK-0333 6e-0, ADR-0085, RFC-0100 §3)
//! against the real syscall table: a task binds its endpoint (`SYSCALL_MEM_PRESSURE_BIND`) and
//! the level of the moment arrives as one `OP_MEMORY_PRESSURE` frame with the pool's free and
//! total frames; an injected `warn` arrives as the next frame; a second process's bind is
//! refused with `EPERM` while the first holds the sink; the holder's clear frees it. A real
//! level change needs a pool run low — the `lowmem` lane of 6f; the delivery path it would take
//! is the one proven here. The purge (RFC-0100 §4, 6e-3) rides the same delivery: a lazy object
//! its owner unlocked loses its pages when `warn` is delivered and says so at the lock; a locked
//! object, a device-bound one, an eager one and a mapped one are never touched.
//! OWNERS: @kernel-mm-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `KSELFTEST: mem pressure notify ok (…)`, `KSELFTEST: vmo purge ok (…)`, every
//!   QEMU lane and the board ladder

use super::Context;
use crate::cap::{CapError, Capability, CapabilityKind, Rights};
use crate::mm::pressure_endpoint;
use crate::pressure::{self, Level};
use crate::syscall::{api, Args, Error as SysError, SyscallTable, SYSCALL_MEM_PRESSURE_BIND};
use crate::task::Pid;

/// Bootstrap cap slot above the process-ctl selftest's (55).
const SINK_SLOT: usize = 56;

fn bind(table: &SyscallTable, ctx: &mut api::Context<'_>, slot: usize) -> Result<usize, SysError> {
    table.dispatch(SYSCALL_MEM_PRESSURE_BIND, ctx, &Args::new([slot, 0, 0, 0, 0, 0]))
}

/// The one frame waiting on `ep`, decoded.
fn heard(router: &mut crate::ipc::Router, ep: u32) -> Option<(Level, u64, u64)> {
    router.recv(ep).ok().and_then(|msg| pressure::decode(msg.payload.as_slice()))
}

pub(super) fn run_pressure_selftests(ctx: &mut Context<'_>) {
    ctx.tasks.set_current(Pid::KERNEL);
    let holder = ctx.tasks.selftest_create_dummy_task(Pid::KERNEL, ctx.scheduler);
    let stranger = ctx.tasks.selftest_create_dummy_task(Pid::KERNEL, ctx.scheduler);
    let mut sinks = [0u32; 2];
    for (pid, sink) in [holder, stranger].into_iter().zip(sinks.iter_mut()) {
        let Ok(ep) = ctx.router.create_endpoint(4, Some(pid.as_raw())) else {
            log_error!(target: "selftest", "KSELFTEST: mem pressure notify FAIL (create)");
            return;
        };
        let cap =
            Capability { kind: CapabilityKind::Endpoint(ep), rights: Rights::RECV | Rights::SEND };
        if !ctx.tasks.caps_of_mut(pid).is_some_and(|caps| caps.set(SINK_SLOT, cap).is_ok()) {
            log_error!(target: "selftest", "KSELFTEST: mem pressure notify FAIL (caps)");
            return;
        }
        *sink = ep;
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
    // The holder binds: the level of the moment is the first frame, with the pool's counts.
    sys.tasks.set_current(holder);
    let bound = bind(&table, &mut sys, SINK_SLOT);
    pressure_endpoint::dispatch(sys.router, sys.tasks, sys.scheduler);
    let first = heard(sys.router, sinks[0]);
    // A change: `warn` left pending as the pool would, delivered on the next dispatch.
    pressure_endpoint::selftest_inject(Level::Warn);
    pressure_endpoint::dispatch(sys.router, sys.tasks, sys.scheduler);
    let second = heard(sys.router, sinks[0]);
    // A stranger is refused while the holder holds the sink; its endpoint hears nothing.
    sys.tasks.set_current(stranger);
    let refused = bind(&table, &mut sys, SINK_SLOT);
    pressure_endpoint::dispatch(sys.router, sys.tasks, sys.scheduler);
    let stranger_heard = heard(sys.router, sinks[1]);
    // The holder clears; the sink is free for the service of record (memd).
    sys.tasks.set_current(holder);
    let cleared = bind(&table, &mut sys, usize::MAX);
    let free_again = pressure_endpoint::binding().is_none() && !pressure_endpoint::pending();
    sys.tasks.set_current(Pid::KERNEL);
    let denied = SysError::Capability(CapError::PermissionDenied);
    match (bound, first, second, refused, stranger_heard, cleared, free_again) {
        (
            Ok(0),
            Some((level, free, total)),
            Some((Level::Warn, _, _)),
            Err(e),
            None,
            Ok(0),
            true,
        ) if e == denied && total > 0 && free <= total => {
            log_info!(
                target: "selftest",
                "KSELFTEST: mem pressure notify ok (level={} then=warn reject=denied free={} total={})",
                level.name(),
                free,
                total
            );
        }
        (bound, first, second, refused, stranger_heard, cleared, free_again) => log_error!(
            target: "selftest",
            "KSELFTEST: mem pressure notify FAIL (bind={:?} first={:?} second={:?} refused={:?} stranger={:?} cleared={:?} free={})",
            bound,
            first,
            second,
            refused,
            stranger_heard,
            cleared,
            free_again
        ),
    }
}

/// RFC-0100 §4: a lazy object its owner unlocked loses its pages when `warn` is delivered —
/// through the same dispatch the sink is served by — and says so at the lock; the unlock is
/// refused for a device-bound, an eager and a mapped object; a locked object keeps its pages.
pub(super) fn run_purge_selftest(ctx: &mut Context<'_>) {
    use crate::mm::vmo::{self, purge, VmoKind};
    use crate::mm::PAGE_SIZE;
    use crate::vmo_lifetime::PurgeRefusal;
    const PAGES: usize = 4;
    let reach = crate::dma_reach::DmaReach::ALL;
    let (Ok(cache), Ok(bound), Ok(eager)) = (
        vmo::create(PAGES * PAGE_SIZE, VmoKind::Lazy, &reach),
        vmo::create(PAGE_SIZE, VmoKind::Lazy, &reach),
        vmo::create(PAGE_SIZE, VmoKind::Anon, &reach),
    ) else {
        log_error!(target: "selftest", "KSELFTEST: vmo purge FAIL (create)");
        return;
    };
    let committed =
        (0..PAGES).all(|i| vmo::commit(cache, i).is_ok()) && vmo::commit(bound, 0).is_ok();
    // A device was told the bound object's runs (a GPU binding is the same fact).
    vmo::mark_device_named(bound);
    let reject_bound = purge::unlock(bound, false);
    let reject_eager = purge::unlock(eager, false);
    let reject_mapped = purge::unlock(cache, true);
    // Locked, a purge takes nothing from it.
    let kept = purge::purge_unlocked() == 0
        && vmo::committed_in(cache, 0, PAGES * PAGE_SIZE) == PAGES * PAGE_SIZE;
    let unlocked = purge::unlock(cache, false);
    let before = purge::purged_pages();
    // The level change delivers the purge (ADR-0085's path), not a call of this test's.
    pressure_endpoint::selftest_inject(Level::Warn);
    pressure_endpoint::dispatch(ctx.router, ctx.tasks, ctx.scheduler);
    let dropped = purge::purged_pages().saturating_sub(before);
    let empty = vmo::committed_in(cache, 0, PAGES * PAGE_SIZE) == 0;
    let relocked = purge::lock(cache);
    let bound_kept = vmo::committed_in(bound, 0, PAGE_SIZE) == PAGE_SIZE;
    for id in [cache, bound, eager] {
        let _ = vmo::destroy(id);
    }
    let refused = |refusal| Err(purge::PurgeError::Refused(refusal));
    if committed
        && reject_bound == refused(PurgeRefusal::DeviceBound)
        && reject_eager == refused(PurgeRefusal::Kind)
        && reject_mapped == refused(PurgeRefusal::Mapped)
        && kept
        && unlocked == Ok(())
        && dropped == PAGES as u64
        && empty
        && relocked == Ok(true)
        && bound_kept
    {
        log_info!(
            target: "selftest",
            "KSELFTEST: vmo purge ok (pages={} purged={} relocked=purged reject=bound,eager,mapped)",
            PAGES,
            dropped
        );
    } else {
        log_error!(
            target: "selftest",
            "KSELFTEST: vmo purge FAIL (committed={} bound={:?} eager={:?} mapped={:?} kept={} unlocked={:?} dropped={} empty={} relocked={:?} bound_kept={})",
            committed,
            reject_bound,
            reject_eager,
            reject_mapped,
            kept,
            unlocked,
            dropped,
            empty,
            relocked,
            bound_kept
        );
    }
}
