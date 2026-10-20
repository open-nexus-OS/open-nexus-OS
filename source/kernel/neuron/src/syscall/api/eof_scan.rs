// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The ONE "who still names this endpoint" scan behind RFC-0079 last-sender EOF
//! (TASK-0324 P7-b) and its two siblings of TASK-0333 6a — the sender's `PeerClosed` once no
//! RECV capability exists, and the endpoint's death once no capability exists at all. Four
//! paths ask it — a blocking or non-blocking EOF-opted recv, a `cap_close` that dropped an
//! endpoint capability, and a task exit that dropped every capability it held — and all must
//! agree, or a receiver blocks forever on a peer that is gone. The decisions are the
//! host-tested predicates in `crate::ipc_eof`; this module gathers the per-task counts in ONE
//! pass per endpoint (`census`: senders, receivers and names together — three passes per
//! close cost 8 % of an IPC call on the headless lane, measured 2026-10-20). The endpoint's
//! OWNER is excluded as a sender: it keeps the SEND base of its own reply channel to clone one
//! per exchange, and is never its own peer; it counts as a receiver like anyone, a receive
//! needs the capability whoever holds it.
//! OWNERS: @kernel-ipc-team
//! STATUS: Functional
//! API_STABILITY: Unstable

use super::*;

/// True while some task OTHER than the endpoint's owner holds a live SEND cap to `endpoint`
/// (the recv path's question; one pass).
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

/// RFC-0079: what an empty queue means for this receiver — the one scan every EOF path shares
/// feeds the one host-tested verdict (`ipc_eof`). A live peer also latches the endpoint
/// (belt-and-suspenders with the send path).
pub(super) fn empty_queue_verdict(
    tasks: &task::TaskTable,
    router: &mut ipc::Router,
    endpoint: ipc::EndpointId,
    nonblock: bool,
    eof_opt: bool,
) -> crate::ipc_eof::EmptyQueue {
    let any_sender = foreign_sender_remains(tasks, router, endpoint);
    if any_sender {
        router.mark_endpoint_had_sender(endpoint);
    }
    let had_sender = router.endpoint_had_sender(endpoint);
    crate::ipc_eof::empty_queue_verdict(nonblock, eof_opt, had_sender, any_sender)
}

/// Who still names `endpoint`, counted in one pass over the tables and one over the queues.
pub(super) struct Census {
    /// A task other than the owner holds a SEND capability, or one is in flight.
    pub foreign_sender: bool,
    /// Some task holds a RECV capability, or one is in flight.
    pub receiver: bool,
    /// Any capability at all names the endpoint, in a table or in flight.
    pub named: bool,
}

pub(super) fn census(
    tasks: &task::TaskTable,
    router: &ipc::Router,
    endpoint: ipc::EndpointId,
) -> Census {
    let owner = router.endpoint_owner(endpoint);
    let (flight_send, flight_recv, flight_any) = router.in_flight_census(endpoint);
    let (mut foreign_send, mut recv, mut any) = (0usize, 0usize, flight_any);
    for raw in 0..tasks.len() as u32 {
        let Some(caps) = tasks.caps_of(task::Pid::from_raw(raw)) else { continue };
        let (s, r, a) = caps.endpoint_census(endpoint);
        // The owner is never its own peer (TASK-0324 P7-b); a cap in flight always is.
        if Some(raw) != owner {
            foreign_send += s;
        }
        recv += r;
        any += a;
    }
    Census {
        foreign_sender: flight_send > 0 || foreign_send > 0,
        receiver: crate::ipc_eof::receiver_remains(flight_recv, core::iter::once(recv)),
        named: any > 0,
    }
}

/// A capability to `endpoint` was dropped (a close, an exit): one census, three verdicts.
/// Receivers whose last peer left wake to `PeerClosed` (RFC-0079); senders whose last receiver
/// left likewise (the mirror); an endpoint nothing names dies and is forgotten by every
/// waitset, timer, interrupt line and exit binding (RFC-0100 §1). Returns whether it died.
pub(super) fn judge_dropped(
    tasks: &mut task::TaskTable,
    router: &mut ipc::Router,
    scheduler: &mut Scheduler,
    waitsets: &mut crate::waitset::WaitsetTable,
    hart_timers: &mut crate::timer::HartTimers,
    endpoint: ipc::EndpointId,
    by: &'static str,
) -> bool {
    if !router.endpoint_alive(endpoint) {
        return false;
    }
    let census = census(tasks, router, endpoint);
    if !census.named {
        return super::ipc_reclaim::destroy(tasks, router, waitsets, hart_timers, endpoint, by);
    }
    if !census.foreign_sender {
        // Latched for waitset waiters (they wake through the recv-waiter drain like any
        // receiver and find the member READY instead of re-parking); latched whether or not a
        // sender was ever seen — the receive that follows decides EOF by the live rule
        // (`had_sender` included), the latch only ENDS the wait (P8: init's responder).
        router.set_eof_pending(endpoint);
        for pid in router.drain_recv_waiters(endpoint) {
            observe_wake_outcome(tasks.wake(task::Pid::from_raw(pid), scheduler));
        }
    }
    if !census.receiver {
        for pid in router.mark_receiver_gone(endpoint) {
            observe_wake_outcome(tasks.wake(task::Pid::from_raw(pid), scheduler));
        }
    }
    false
}
