// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The ONE "who still holds this endpoint" answer behind RFC-0079 last-sender EOF
//! (TASK-0324 P7-b) and its two siblings of TASK-0333 6a — the sender's `PeerClosed` once no
//! RECV capability exists, and the endpoint's death once no capability exists at all. Four
//! paths ask it — a blocking or non-blocking EOF-opted recv, a `cap_close` that dropped an
//! endpoint capability, and a task exit that dropped every capability it held — and all must
//! agree, or a receiver blocks forever on a peer that is gone. The decisions are the
//! host-tested predicates in `crate::ipc_eof`; the answer is read from the endpoint's header,
//! which counts every capability held to it where it is made and dropped (`cap/held.rs`,
//! RFC-0102) — until TASK-0330 P5 this module walked every slot of every task's table for it,
//! under the kernel lock, on every blocking receive and every close (58 % of the lock's time
//! on the board, 2026-11-11). The endpoint's OWNER is excluded as a sender: it keeps the SEND
//! base of its own reply channel to clone one per exchange, and is never its own peer; it
//! counts as a receiver like anyone, a receive needs the capability whoever holds it.
//! OWNERS: @kernel-ipc-team
//! STATUS: Functional
//! API_STABILITY: Unstable

use super::*;

/// True while a SEND capability to `endpoint` is held outside its owner's table — by another
/// task, or in a message still queued (whoever dequeues it will hold it).
pub(super) fn foreign_sender_remains(router: &ipc::Router, endpoint: ipc::EndpointId) -> bool {
    router.held_counts(endpoint).is_some_and(|counts| counts.foreign_senders() > 0)
}

/// RFC-0079: what an empty queue means for this receiver — the one answer every EOF path
/// shares feeds the one host-tested verdict (`ipc_eof`). A live peer also latches the
/// endpoint (belt-and-suspenders with the send path).
pub(super) fn empty_queue_verdict(
    router: &mut ipc::Router,
    endpoint: ipc::EndpointId,
    nonblock: bool,
    eof_opt: bool,
) -> crate::ipc_eof::EmptyQueue {
    let any_sender = foreign_sender_remains(router, endpoint);
    if any_sender {
        router.mark_endpoint_had_sender(endpoint);
    }
    let had_sender = router.endpoint_had_sender(endpoint);
    crate::ipc_eof::empty_queue_verdict(nonblock, eof_opt, had_sender, any_sender)
}

/// Who still holds `endpoint`, read from its header.
pub(super) struct Standing {
    /// A SEND capability is held outside the owner's table, or one is in flight.
    pub foreign_sender: bool,
    /// A RECV capability is held, in a table or in flight.
    pub receiver: bool,
    /// Any capability at all is held to the endpoint, in a table or in flight.
    pub held: bool,
}

pub(super) fn standing(router: &ipc::Router, endpoint: ipc::EndpointId) -> Standing {
    match router.held_counts(endpoint) {
        Some(counts) => Standing {
            foreign_sender: counts.foreign_senders() > 0,
            receiver: counts.receivers() > 0,
            held: counts.any() > 0,
        },
        None => Standing { foreign_sender: false, receiver: false, held: false },
    }
}

/// A capability to `endpoint` was dropped (a close, an exit): one reading, three verdicts.
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
    let standing = standing(router, endpoint);
    if !standing.held {
        return super::ipc_reclaim::destroy(tasks, router, waitsets, hart_timers, endpoint, by);
    }
    if !standing.foreign_sender {
        // Latched for waitset waiters (they wake through the recv-waiter drain like any
        // receiver and find the member READY instead of re-parking); latched whether or not a
        // sender was ever seen — the receive that follows decides EOF by the live rule
        // (`had_sender` included), the latch only ENDS the wait (P8: init's responder).
        router.set_eof_pending(endpoint);
        for pid in router.drain_recv_waiters(endpoint) {
            observe_wake_outcome(tasks.wake(task::Pid::from_raw(pid), scheduler));
        }
    }
    if !standing.receiver {
        for pid in router.mark_receiver_gone(endpoint) {
            observe_wake_outcome(tasks.wake(task::Pid::from_raw(pid), scheduler));
        }
    }
    false
}

/// RFC-0112 (amendment 2026-11-02): the capabilities that were IN FLIGHT in the queue of an
/// endpoint that closed — its owner died, a MANAGE close, a reclaim — are dropped like a
/// `cap_close` of each: taken out of the router's list it is counted off, so an endpoint one of
/// them held may have lost its last foreign sender (its receiver wakes to EOF: the caller of a
/// call the dead server never read), its last receiver or its last capability; a VMO its last
/// capability. Drained
/// to the end: a judgement may destroy an endpoint whose own queue held more. Timers, waitsets
/// and fences die with their creator; a dropped capability to one only loses a name.
pub(super) fn release_in_flight(
    tasks: &mut task::TaskTable,
    router: &mut ipc::Router,
    scheduler: &mut Scheduler,
    (waitsets, hart_timers): (&mut crate::waitset::WaitsetTable, &mut crate::timer::HartTimers),
    address_spaces: &AddressSpaceManager,
    by: &'static str,
) {
    while let Some(cap) = router.take_in_flight_dropped() {
        match cap.kind {
            CapabilityKind::Endpoint(id) => {
                let _ = judge_dropped(tasks, router, scheduler, waitsets, hart_timers, id, by);
            }
            CapabilityKind::Vmo { id, .. } | CapabilityKind::VmoRo { id, .. } => {
                crate::mm::reclaim::orphans(address_spaces, &[id], by);
            }
            _ => {}
        }
    }
}
