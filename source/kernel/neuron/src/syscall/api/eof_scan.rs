// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The ONE "does a peer still hold a SEND cap to this endpoint" scan behind RFC-0079
//! last-sender EOF (TASK-0324 P7-b). Three paths ask it — a blocking EOF-opted recv, a
//! `cap_close` that dropped a SEND cap, and a task exit that dropped every cap it held — and
//! all three must agree, or a receiver blocks forever on a peer that is gone. The decision
//! itself is the host-tested predicate in `crate::ipc_eof`; this module only gathers the
//! per-task counts. The endpoint's OWNER is excluded: it keeps the SEND base of its own
//! reply channel to clone one per exchange, and is never its own peer.
//! OWNERS: @kernel-ipc-team
//! STATUS: Functional
//! API_STABILITY: Unstable

use super::*;

/// True while some task OTHER than the endpoint's owner holds a live SEND cap to `endpoint`.
pub(super) fn foreign_sender_remains(
    tasks: &task::TaskTable,
    router: &ipc::Router,
    endpoint: ipc::EndpointId,
) -> bool {
    // A SEND cap moved inside a still-queued message belongs to the peer that will dequeue
    // it — never the owner — so it counts as a foreign sender outright.
    if router.in_flight_send_cap_count(endpoint) > 0 {
        return true;
    }
    let owner = router.endpoint_owner(endpoint);
    let holders = (0..tasks.len() as u32).filter_map(|raw| {
        tasks
            .caps_of(task::Pid::from_raw(raw))
            .map(|caps| (raw, caps.endpoint_send_cap_count(endpoint)))
    });
    crate::ipc_eof::foreign_sender_remains(owner, holders)
}

/// Wakes every receiver blocked on `endpoint` when no peer remains, so an EOF-opted recv
/// re-runs and returns `PeerClosed`. No-op while a peer still holds a SEND cap.
pub(super) fn wake_receivers_if_last_peer_gone(
    tasks: &mut task::TaskTable,
    router: &mut ipc::Router,
    scheduler: &mut Scheduler,
    endpoint: ipc::EndpointId,
) {
    if foreign_sender_remains(tasks, router, endpoint) {
        return;
    }
    // No foreign sender remains: latch it for waitset waiters (they wake through the recv-waiter
    // drain below like any receiver, and find the member READY instead of re-parking). Latched
    // whether or not a sender was ever seen — the receive that follows decides EOF by the live
    // rule (`had_sender` included); the latch only ENDS the wait, so a supervisor parked on a
    // waitset learns of a child that died before it ever wrote (P8: init's responder).
    router.set_eof_pending(endpoint);
    for pid in router.drain_recv_waiters(endpoint) {
        observe_wake_outcome(tasks.wake(task::Pid::from_raw(pid), scheduler));
    }
}
