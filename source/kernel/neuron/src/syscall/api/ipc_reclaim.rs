// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: An endpoint dies with its last capability (RFC-0100 §1 applied to IPC, TASK-0333
//! 6a) — the sibling of `mm::reclaim` for the router. The rule is the VMO's: an object nothing
//! names (no capability in any table, none moving inside a queued message) is destroyed, and
//! everything that remembered its id forgets it — waitset member lists, timers notifying it,
//! interrupt lines bound to it, parents that bound it for their children's deaths — so the
//! slot can carry a new endpoint without a stale name reaching it. The judgement is one
//! reading of the endpoint's header per dropped capability (`eof_scan::judge_dropped`, shared
//! with the EOF wakes; the header counts what is held, RFC-0102); this
//! module is the destroy, the forget, the record and the slot reuse. Measured before this:
//! two endpoints per app launch, owned by execd, closed by nobody
//! (`docs/board/measurements/2026-10-20-memory-lifecycle-baseline/`).
//! OWNERS: @kernel-ipc-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `KSELFTEST: task exit releases ok (…)` each boot; the rule's truth table on
//!   the host (`crate::ipc_eof`); `SELFTEST: rss loop ok (… kobjects=0)`

use core::sync::atomic::{AtomicUsize, Ordering};

use super::*;

/// Endpoints destroyed by this path since boot (`KSELFTEST: vm reclaim (… endpoints=)`).
static RECLAIMED: AtomicUsize = AtomicUsize::new(0);

/// Reclaims named on the console before the line goes quiet (the first ones of a boot).
const NAMED_RECLAIMS: usize = 24;

/// Everything that remembered a now-dead `id` forgets it. Called for every endpoint death,
/// whichever path killed it (the reclaim, the owner's exit, a MANAGE close).
///
/// Each of them behind its own lock, taken alone (TASK-0330 P4): no kernel lock, no task
/// table. The caller holds the dead endpoint's header until this returns — its id goes to no
/// new endpoint while a waitset, a timer or a line still names it (`ipc::lifetime`). A parent
/// that bound it for its children's deaths holds the header itself and is not asked.
pub(super) fn forget_dead(
    waitsets: &crate::waitset::WaitsetTable,
    hart_timers: &crate::timer::HartTimers,
    id: ipc::EndpointId,
) {
    let _ = waitsets.forget_endpoint(id);
    let _ = hart_timers.forget_endpoint(id);
    let _ = crate::irq::forget_endpoint(id);
    crate::mm::pressure_endpoint::forget_endpoint(id);
}

/// A locked path stored the id of `pin`'s endpoint — a waitset member, a timer's notify
/// endpoint, an interrupt line, the pressure sink — while a closer off the kernel lock may kill
/// it (TASK-0330 P4). Read after the store, with the pin still held: a closer marks the
/// endpoint dead before it forgets it (`destroy`), both under locks this store and this read
/// take too, so either this reads it dead and forgets it here — the new store included — or
/// the closer's forget comes after the store and finds it. The pin keeps the id the endpoint's
/// meanwhile (`ipc::lifetime`). Returns whether it lives.
pub(super) fn stored(
    router: &ipc::Router,
    waitsets: &crate::waitset::WaitsetTable,
    hart_timers: &crate::timer::HartTimers,
    pin: &crate::cap::PinnedEndpoint,
) -> bool {
    let alive = router.endpoint_alive(pin.id());
    if !alive {
        forget_dead(waitsets, hart_timers, pin.id());
    }
    alive
}

/// Destroys `id` — nothing holds a capability to it — and forgets it everywhere; `by`
/// names the caller (`exit`, `close`) on the console. Returns whether it died. The caller
/// holds the endpoint's header (`cap::Dropped`, a pin) — see `forget_dead`.
pub(super) fn destroy(
    router: &ipc::Router,
    waitsets: &crate::waitset::WaitsetTable,
    hart_timers: &crate::timer::HartTimers,
    id: ipc::EndpointId,
    by: &'static str,
) -> bool {
    // Two closers that each dropped a last capability: one of them closes it, the other's
    // close finds it dead.
    let Ok(waiters) = router.close_endpoint(id) else {
        return false;
    };
    forget_dead(waitsets, hart_timers, id);
    // A thread parked on it while a thread of its process closed the last capability under
    // it: it runs its syscall again and finds the slot empty.
    for pid in waiters {
        super::ipc_ctx::wake(pid);
    }
    let n = RECLAIMED.fetch_add(1, Ordering::Relaxed);
    if n < NAMED_RECLAIMS {
        log_info!(
            target: "ipc",
            "KINIT: ipc reclaim (id={} by={} endpoints={})",
            id,
            by,
            router.live_endpoint_count()
        );
    }
    true
}

/// Endpoints destroyed by this path since boot.
pub fn stats() -> usize {
    RECLAIMED.load(Ordering::Relaxed)
}

/// A create that prefers a dead slot no capability is held to any more (the table and the
/// per-owner quota were a launch counter while ids were never reused); a fresh slot otherwise.
pub(super) fn create_endpoint(
    ctx: &mut Context<'_>,
    depth: usize,
    owner: Option<u32>,
) -> Result<ipc::EndpointId, ipc::IpcError> {
    match ctx.router.reusable_dead_endpoint() {
        Some(id) => ctx.router.revive_endpoint(id, depth, owner),
        None => ctx.router.create_endpoint(depth, owner),
    }
}
