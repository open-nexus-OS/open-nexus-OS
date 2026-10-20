// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: An endpoint dies with its last capability (RFC-0100 §1 applied to IPC, TASK-0333
//! 6a) — the sibling of `mm::reclaim` for the router. The rule is the VMO's: an object nothing
//! names (no capability in any table, none moving inside a queued message) is destroyed, and
//! everything that remembered its id forgets it — waitset member lists, timers notifying it,
//! interrupt lines bound to it, parents that bound it for their children's deaths — so the
//! slot can carry a new endpoint without a stale name reaching it. The judgement is one
//! census per dropped capability (`eof_scan::judge_dropped`, shared with the EOF wakes); this
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
pub(super) fn forget_dead(
    tasks: &mut task::TaskTable,
    waitsets: &mut crate::waitset::WaitsetTable,
    hart_timers: &mut crate::timer::HartTimers,
    id: ipc::EndpointId,
) {
    let _ = waitsets.forget_endpoint(id);
    let _ = hart_timers.forget_endpoint(id);
    let _ = crate::irq::forget_endpoint(id);
    tasks.forget_exit_notify(id);
}

/// Destroys `id` — the census found nothing naming it — and forgets it everywhere; `by`
/// names the caller (`exit`, `close`) on the console. Returns whether it died (a blocked peer
/// of an unnamed endpoint cannot exist, so its drained waiters need no wake).
pub(super) fn destroy(
    tasks: &mut task::TaskTable,
    router: &mut ipc::Router,
    waitsets: &mut crate::waitset::WaitsetTable,
    hart_timers: &mut crate::timer::HartTimers,
    id: ipc::EndpointId,
    by: &'static str,
) -> bool {
    if router.close_endpoint(id).is_err() {
        return false;
    }
    forget_dead(tasks, waitsets, hart_timers, id);
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

/// A create that prefers a dead slot nothing names any more (the table and the per-owner
/// quota were a launch counter while ids were never reused); a fresh slot otherwise.
pub(super) fn create_endpoint(
    ctx: &mut Context<'_>,
    depth: usize,
    owner: Option<u32>,
) -> Result<ipc::EndpointId, ipc::IpcError> {
    let mut dead = alloc::vec::Vec::new();
    ctx.router.dead_endpoint_ids(&mut dead);
    for id in dead {
        if !super::eof_scan::census(ctx.tasks, ctx.router, id).named {
            return ctx.router.revive_endpoint(id, depth, owner);
        }
    }
    ctx.router.create_endpoint(depth, owner)
}
